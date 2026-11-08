//! The single exclusion policy used by every enumeration, full or partial (F04-AC5, ADR-0006).
//!
//! A rule matches either the **exact name** of a folder (anywhere in the volume) or the
//! **exact path** of a file or folder relative to the volume root. Names are compared without
//! distinguishing upper and lower case, as Windows does. A rule never matches a substring:
//! excluding `Temp` does not exclude `Templates` or `C:\Temp2`.

use crate::names;

/// One exclusion rule.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExclusionRule {
    /// Any folder with exactly this name, at any depth.
    DirName(String),
    /// Exactly this path relative to the volume root, components separated by `\` (or `/`).
    /// A leading separator is optional: `\$RECYCLE.BIN` and `$RECYCLE.BIN` are the same rule.
    Path(String),
}

/// A set of exclusion rules, compiled for fast checks during enumeration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExclusionPolicy {
    rules: Vec<ExclusionRule>,
    /// Case-folded folder names.
    dir_names: Vec<String>,
    /// Case-folded path components of each path rule (never empty).
    paths: Vec<Vec<String>>,
}

/// Where a folder sits with respect to the path rules: the indexes of the path rules whose
/// leading components match the folder's path. Obtained from [`ExclusionPolicy::root_cursor`]
/// and [`ExclusionPolicy::child`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleCursor {
    depth: usize,
    active: Vec<u16>,
}

/// Result of checking one child of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChildCheck {
    Excluded,
    /// Included; for folders, the cursor to use for their own children.
    Included(RuleCursor),
}

impl ExclusionPolicy {
    /// The default rules decided for Prism (SPEC Q2): only the Recycle Bin and
    /// `System Volume Information`, at the root of each volume.
    #[must_use]
    pub fn default_rules() -> Vec<ExclusionRule> {
        vec![
            ExclusionRule::Path("$RECYCLE.BIN".to_owned()),
            ExclusionRule::Path("System Volume Information".to_owned()),
        ]
    }

    /// The default policy (see [`ExclusionPolicy::default_rules`]).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(Self::default_rules())
    }

    /// Builds a policy. Empty rules are ignored.
    #[must_use]
    pub fn new(rules: impl IntoIterator<Item = ExclusionRule>) -> Self {
        let mut policy = Self::default();
        for rule in rules {
            match &rule {
                ExclusionRule::DirName(name) => {
                    if name.is_empty() {
                        continue;
                    }
                    policy.dir_names.push(names::fold(name));
                }
                ExclusionRule::Path(path) => {
                    let components: Vec<String> = path
                        .split(['\\', '/'])
                        .filter(|c| !c.is_empty())
                        .map(names::fold)
                        .collect();
                    if components.is_empty() {
                        continue;
                    }
                    policy.paths.push(components);
                }
            }
            policy.rules.push(rule);
        }
        policy
    }

    /// The rules this policy was built from.
    #[must_use]
    pub fn rules(&self) -> &[ExclusionRule] {
        &self.rules
    }

    /// Cursor for the root folder of a volume.
    #[must_use]
    pub fn root_cursor(&self) -> RuleCursor {
        RuleCursor {
            depth: 0,
            active: (0..self.paths.len())
                .filter_map(|i| u16::try_from(i).ok())
                .collect(),
        }
    }

    /// Cursor for a folder given by its components relative to the volume root (used to start
    /// a partial enumeration below the root).
    #[must_use]
    pub fn cursor_for<S: AsRef<str>>(&self, components: &[S]) -> Option<RuleCursor> {
        let mut cursor = self.root_cursor();
        for c in components {
            match self.child(&cursor, c.as_ref(), true) {
                ChildCheck::Excluded => return None,
                ChildCheck::Included(next) => cursor = next,
            }
        }
        Some(cursor)
    }

    /// Checks one child (`name`) of the folder described by `parent`.
    #[must_use]
    pub fn child(&self, parent: &RuleCursor, name: &str, is_dir: bool) -> ChildCheck {
        if parent.active.is_empty() && (!is_dir || self.dir_names.is_empty()) {
            // Fast path: nothing can match, no folding needed.
            return ChildCheck::Included(RuleCursor::default_at(parent.depth + 1));
        }
        let folded = names::fold(name);
        if is_dir && self.dir_names.contains(&folded) {
            return ChildCheck::Excluded;
        }
        let mut active = Vec::new();
        for &i in &parent.active {
            let Some(rule) = self.paths.get(usize::from(i)) else {
                continue;
            };
            if rule.get(parent.depth) == Some(&folded) {
                if rule.len() == parent.depth + 1 {
                    return ChildCheck::Excluded;
                }
                if is_dir {
                    active.push(i);
                }
            }
        }
        ChildCheck::Included(RuleCursor {
            depth: parent.depth + 1,
            active,
        })
    }

    /// Whether the entry at `components` (relative to the volume root) is excluded, either
    /// itself or because one of its ancestors is.
    #[must_use]
    pub fn is_excluded<S: AsRef<str>>(&self, components: &[S], is_dir: bool) -> bool {
        let Some((last, parents)) = components.split_last() else {
            return false;
        };
        match self.cursor_for(parents) {
            None => true,
            Some(cursor) => self.child(&cursor, last.as_ref(), is_dir) == ChildCheck::Excluded,
        }
    }
}

impl RuleCursor {
    fn default_at(depth: usize) -> Self {
        Self {
            depth,
            active: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn excluded(policy: &ExclusionPolicy, path: &str, is_dir: bool) -> bool {
        let components: Vec<&str> = path.split('\\').filter(|c| !c.is_empty()).collect();
        policy.is_excluded(&components, is_dir)
    }

    // F04-AC5: defaults exclude only the Recycle Bin and System Volume Information at the root.
    #[test]
    fn f04_ac5_default_rules() {
        let p = ExclusionPolicy::with_defaults();
        assert!(excluded(&p, r"$RECYCLE.BIN", true));
        assert!(excluded(&p, r"$Recycle.Bin", true));
        assert!(excluded(&p, r"$Recycle.Bin\S-1-5-21\file.txt", false));
        assert!(excluded(&p, r"System Volume Information", true));
        assert!(!excluded(&p, r"Users\me\$Recycle.Bin", true));
        assert!(!excluded(&p, r"Windows", true));
        assert!(!excluded(&p, r"node_modules", true));
        assert!(!excluded(&p, r"Templates", true));
    }

    // F04-AC5: rules never match substrings (the old Prism excluded `...\Templates\` by mistake).
    #[test]
    fn f04_ac5_no_substring_matches() {
        let p = ExclusionPolicy::new([
            ExclusionRule::DirName("Temp".into()),
            ExclusionRule::Path(r"\Data\cache".into()),
        ]);
        assert!(excluded(&p, r"Temp", true));
        assert!(excluded(&p, r"Users\me\AppData\Local\temp", true));
        assert!(!excluded(&p, r"Templates", true));
        assert!(!excluded(&p, r"Temp2", true));
        assert!(!excluded(&p, r"MyTemp", true));
        // A folder-name rule does not match files.
        assert!(!excluded(&p, r"Docs\Temp", false));
        assert!(excluded(&p, r"Data\Cache", true));
        assert!(excluded(&p, r"data\CACHE\x\y.bin", false));
        assert!(!excluded(&p, r"Data\cache2", true));
        assert!(!excluded(&p, r"Other\Data\cache", true));
        assert!(!excluded(&p, r"Data", true));
    }

    #[test]
    fn path_rule_can_exclude_a_file() {
        let p = ExclusionPolicy::new([ExclusionRule::Path("pagefile.sys".into())]);
        assert!(excluded(&p, "pagefile.sys", false));
        assert!(!excluded(&p, r"x\pagefile.sys", false));
    }

    #[test]
    fn empty_rules_are_ignored() {
        let p = ExclusionPolicy::new([
            ExclusionRule::DirName(String::new()),
            ExclusionRule::Path(r"\\".into()),
        ]);
        assert!(p.rules().is_empty());
        assert!(!excluded(&p, "anything", true));
    }

    #[test]
    fn cursor_walk_matches_is_excluded() {
        let p = ExclusionPolicy::new([ExclusionRule::Path(r"a\b\c".into())]);
        let root = p.root_cursor();
        let ChildCheck::Included(a) = p.child(&root, "A", true) else {
            panic!("a must be included")
        };
        let ChildCheck::Included(b) = p.child(&a, "b", true) else {
            panic!("b must be included")
        };
        assert_eq!(p.child(&b, "C", true), ChildCheck::Excluded);
        assert!(matches!(p.child(&b, "d", true), ChildCheck::Included(_)));
        // A file named like an intermediate component does not keep rules active.
        let ChildCheck::Included(f) = p.child(&root, "a", false) else {
            panic!("file a must be included")
        };
        assert!(f.active.is_empty());
    }
}
