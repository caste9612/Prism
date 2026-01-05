//! Tree view commands for hierarchical file system visualization

use crate::AppState;
use serde::Serialize;
use tauri::State;
use tracing::{debug, info};

/// Node type for tree visualization
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Drive,
    Folder,
    File,
}

/// A node in the directory tree
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    /// Full path to this node
    pub path: String,
    /// Display name (folder/file name)
    pub name: String,
    /// Total size of this node and all children
    pub size: i64,
    /// Number of files in this node (recursively)
    pub file_count: i64,
    /// Type of node (drive, folder, file)
    pub node_type: NodeType,
    /// Depth level in tree (0 = root)
    pub depth: u32,
    /// Children nodes (populated based on max_depth)
    pub children: Vec<TreeNode>,
}

/// Get the directory tree structure up to a specified depth
///
/// This command returns a hierarchical tree structure with aggregated sizes and file counts.
/// For performance with large datasets, uses SQL aggregation and limits depth.
#[tauri::command]
pub async fn get_directory_tree(
    state: State<'_, AppState>,
    root_path: Option<String>,
    max_depth: Option<u32>,
) -> Result<Vec<TreeNode>, String> {
    let max_depth = max_depth.unwrap_or(3);
    info!(
        "Getting directory tree, root: {:?}, max_depth: {}",
        root_path, max_depth
    );

    let db = state.db.lock().await;
    let conn = db.connection();

    // If no root path, get all drives as roots
    let roots = if let Some(ref path) = root_path {
        vec![path.clone()]
    } else {
        // Get distinct drive letters from files table
        let mut stmt = conn
            .prepare("SELECT DISTINCT UPPER(SUBSTR(path, 1, 3)) as drive FROM files ORDER BY drive")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let drives: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Failed to query drives: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        drives
    };

    debug!("Building tree for {} root(s)", roots.len());

    let mut result = Vec::new();

    for root in roots {
        let tree = build_tree_for_root(conn, &root, max_depth)
            .map_err(|e| format!("Failed to build tree for {}: {}", root, e))?;
        if let Some(node) = tree {
            result.push(node);
        }
    }

    info!(
        "Tree built with {} root nodes",
        result.len()
    );

    Ok(result)
}

/// Build tree for a single root path
fn build_tree_for_root(
    conn: &rusqlite::Connection,
    root: &str,
    max_depth: u32,
) -> Result<Option<TreeNode>, rusqlite::Error> {
    // Normalize root path (ensure ends with backslash for drive roots)
    let root_normalized = if root.len() == 3 && root.ends_with(":\\") {
        root.to_uppercase()
    } else if root.len() == 2 && root.ends_with(':') {
        format!("{}\\", root.to_uppercase())
    } else {
        root.to_string()
    };

    let root_pattern = format!("{}%", root_normalized);
    let root_len = root_normalized.len() as i32;

    // First, get the root node stats
    let (total_size, total_files): (i64, i64) = conn
        .query_row(
            "SELECT COALESCE(SUM(size), 0), COUNT(*) FROM files WHERE path LIKE ?1",
            [&root_pattern],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

    if total_files == 0 {
        return Ok(None);
    }

    // Determine node type
    let node_type = if root_normalized.len() == 3 && root_normalized.ends_with(":\\") {
        NodeType::Drive
    } else {
        NodeType::Folder
    };

    let root_name = if node_type == NodeType::Drive {
        root_normalized[..2].to_string() // "C:"
    } else {
        root_normalized
            .trim_end_matches('\\')
            .rsplit('\\')
            .next()
            .unwrap_or(&root_normalized)
            .to_string()
    };

    // Build children recursively
    let children = if max_depth > 0 {
        build_children(conn, &root_normalized, root_len, 1, max_depth)?
    } else {
        Vec::new()
    };

    Ok(Some(TreeNode {
        path: root_normalized,
        name: root_name,
        size: total_size,
        file_count: total_files,
        node_type,
        depth: 0,
        children,
    }))
}

/// Build children for a parent path at a given depth
fn build_children(
    conn: &rusqlite::Connection,
    parent_path: &str,
    parent_len: i32,
    current_depth: u32,
    max_depth: u32,
) -> Result<Vec<TreeNode>, rusqlite::Error> {
    if current_depth > max_depth {
        return Ok(Vec::new());
    }

    let parent_pattern = format!("{}%", parent_path);

    // Get immediate child folders with their aggregated stats
    // This query extracts the next folder level and aggregates
    let query = r#"
        WITH child_paths AS (
            SELECT
                path,
                size,
                CASE
                    WHEN INSTR(SUBSTR(path, ?2 + 1), '\') > 0
                    THEN SUBSTR(path, 1, ?2 + INSTR(SUBSTR(path, ?2 + 1), '\'))
                    ELSE path
                END as child_folder
            FROM files
            WHERE path LIKE ?1 AND LENGTH(path) > ?2
        )
        SELECT
            child_folder,
            SUM(size) as total_size,
            COUNT(*) as file_count
        FROM child_paths
        GROUP BY child_folder
        HAVING child_folder != ?3
        ORDER BY total_size DESC
        LIMIT 100
    "#;

    let mut stmt = conn.prepare_cached(query)?;
    let rows = stmt.query_map(
        rusqlite::params![parent_pattern, parent_len, parent_path],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )?;

    let mut children = Vec::new();

    for row in rows {
        let (child_path, size, file_count) = row?;

        // Skip if this is the same as parent (shouldn't happen but safety check)
        if child_path == parent_path {
            continue;
        }

        // Determine if this is a file or folder
        let is_folder = child_path.ends_with('\\');
        let child_len = child_path.len() as i32;

        let name = child_path
            .trim_end_matches('\\')
            .rsplit('\\')
            .next()
            .unwrap_or(&child_path)
            .to_string();

        let node_type = if is_folder {
            NodeType::Folder
        } else {
            NodeType::File
        };

        // Recursively build children for folders
        let sub_children = if is_folder && current_depth < max_depth {
            build_children(conn, &child_path, child_len, current_depth + 1, max_depth)?
        } else {
            Vec::new()
        };

        children.push(TreeNode {
            path: child_path,
            name,
            size,
            file_count,
            node_type,
            depth: current_depth,
            children: sub_children,
        });
    }

    Ok(children)
}

/// Get folder children for lazy-loading in tree view
/// Returns only immediate children of the specified path
#[tauri::command]
pub async fn get_tree_children(
    state: State<'_, AppState>,
    parent_path: String,
) -> Result<Vec<TreeNode>, String> {
    debug!("Getting tree children for: {}", parent_path);

    let db = state.db.lock().await;
    let conn = db.connection();

    // Ensure path ends with backslash
    let parent_normalized = if parent_path.ends_with('\\') {
        parent_path
    } else {
        format!("{}\\", parent_path)
    };

    let parent_len = parent_normalized.len() as i32;

    let children = build_children(conn, &parent_normalized, parent_len, 1, 1)
        .map_err(|e| format!("Failed to get children: {}", e))?;

    Ok(children)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_node_serialization() {
        let node = TreeNode {
            path: "C:\\Users".to_string(),
            name: "Users".to_string(),
            size: 1024,
            file_count: 10,
            node_type: NodeType::Folder,
            depth: 1,
            children: vec![],
        };

        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("\"nodeType\":\"folder\""));
        assert!(json.contains("\"fileCount\":10"));
    }
}
