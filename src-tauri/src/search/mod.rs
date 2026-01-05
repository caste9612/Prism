//! Search engine module for Prism
//!
//! Provides fast full-text search with query parsing and filtering.

mod parser;
mod executor;
mod cache;

pub use parser::{ParsedQuery, QueryFilter, parse_query};
pub use executor::SearchExecutor;
pub use cache::SearchCache;

use serde::{Deserialize, Serialize};

/// Search result with relevance score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub size: i64,
    pub modified_at: Option<i64>,
    pub rank: f64,
    pub snippet: Option<String>,
}

/// Search options
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub limit: i64,
    pub offset: i64,
    pub sort_by: SortField,
    pub sort_order: SortOrder,
}

#[derive(Debug, Clone, Default)]
pub enum SortField {
    #[default]
    Relevance,
    Name,
    Size,
    Modified,
    Path,
}

#[derive(Debug, Clone, Default)]
pub enum SortOrder {
    #[default]
    Desc,
    Asc,
}
