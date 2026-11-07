//! An in-memory file system with stable file references, standing in for an NTFS volume read
//! through the MFT. It lets the tests exercise move detection by reference (F05-AC4) before
//! the real MFT reader exists (T2), and generate thousands of random cases quickly.

use std::collections::{BTreeMap, HashSet};

use prism_core::model::{ATTR_DIRECTORY, EntryMeta, FileTime};
use prism_core::names;
use prism_core::scan::{
    Enumerator, ScanChild, ScanError, ScanRequest, ScanStats, ScanTree, ScanTreeBuilder,
};

/// File reference of the root folder (as on NTFS).
pub const ROOT: u64 = 5;

#[derive(Clone, Debug)]
pub struct FakeNode {
    pub name: String,
    pub parent: Option<u64>,
    pub is_dir: bool,
    pub size: u64,
    pub modified: i64,
}

#[derive(Clone, Debug)]
pub struct FakeFs {
    nodes: BTreeMap<u64, FakeNode>,
    next_ref: u64,
    clock: i64,
    /// Folders whose listing fails.
    pub unreadable: HashSet<u64>,
}

impl Default for FakeFs {
    fn default() -> Self {
        Self::new()
    }
}

/// A small deterministic random generator (xorshift64*), seeded by proptest.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            items.get(self.below(items.len()))
        }
    }
}

/// Names chosen to collide often, including names that differ only in case.
pub const NAMES: &[&str] = &[
    "a", "b", "c", "A", "Photos", "photos", "doc.txt", "Doc.TXT", "img.jpg", "z", "été", "日本",
];

impl FakeFs {
    pub fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            ROOT,
            FakeNode {
                name: String::new(),
                parent: None,
                is_dir: true,
                size: 0,
                modified: 1,
            },
        );
        Self {
            nodes,
            next_ref: 100,
            clock: 10,
            unreadable: HashSet::new(),
        }
    }

    fn tick(&mut self) -> i64 {
        self.clock += 1;
        self.clock
    }

    fn touch(&mut self, r: u64) {
        let t = self.tick();
        if let Some(n) = self.nodes.get_mut(&r) {
            n.modified = t;
        }
    }

    pub fn node(&self, r: u64) -> Option<&FakeNode> {
        self.nodes.get(&r)
    }

    pub fn exists(&self, r: u64) -> bool {
        self.nodes.contains_key(&r)
    }

    pub fn refs(&self) -> Vec<u64> {
        self.nodes.keys().copied().collect()
    }

    pub fn dirs(&self) -> Vec<u64> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.is_dir)
            .map(|(r, _)| *r)
            .collect()
    }

    pub fn children(&self, dir: u64) -> Vec<u64> {
        let mut c: Vec<(&str, u64)> = self
            .nodes
            .iter()
            .filter(|(_, n)| n.parent == Some(dir))
            .map(|(r, n)| (n.name.as_str(), *r))
            .collect();
        c.sort_unstable();
        c.into_iter().map(|(_, r)| r).collect()
    }

    /// Windows semantics: names are unique per folder without distinguishing case.
    pub fn name_taken(&self, dir: u64, name: &str, except: Option<u64>) -> bool {
        self.nodes.iter().any(|(r, n)| {
            n.parent == Some(dir) && Some(*r) != except && names::eq_ignore_case(&n.name, name)
        })
    }

    pub fn create(&mut self, parent: u64, name: &str, is_dir: bool, size: u64) -> Option<u64> {
        if !self.nodes.get(&parent).is_some_and(|p| p.is_dir) || self.name_taken(parent, name, None)
        {
            return None;
        }
        let r = self.next_ref;
        self.next_ref += 1;
        let t = self.tick();
        self.nodes.insert(
            r,
            FakeNode {
                name: name.to_owned(),
                parent: Some(parent),
                is_dir,
                size: if is_dir { 0 } else { size },
                modified: t,
            },
        );
        self.touch(parent);
        Some(r)
    }

    pub fn find(&self, path: &str) -> Option<u64> {
        let mut cur = ROOT;
        for part in path.split('\\').filter(|p| !p.is_empty()) {
            cur = *self
                .children(cur)
                .iter()
                .find(|c| self.nodes[*c].name == part)?;
        }
        Some(cur)
    }

    pub fn is_ancestor_or_self(&self, a: u64, b: u64) -> bool {
        let mut cur = Some(b);
        while let Some(c) = cur {
            if c == a {
                return true;
            }
            cur = self.nodes.get(&c).and_then(|n| n.parent);
        }
        false
    }

    pub fn depth(&self, r: u64) -> usize {
        let mut d = 0;
        let mut cur = self.nodes.get(&r).and_then(|n| n.parent);
        while let Some(c) = cur {
            d += 1;
            cur = self.nodes.get(&c).and_then(|n| n.parent);
        }
        d
    }

    pub fn path_components(&self, r: u64) -> Vec<String> {
        let mut parts = Vec::new();
        let mut cur = r;
        while let Some(n) = self.nodes.get(&cur) {
            let Some(p) = n.parent else { break };
            parts.push(n.name.clone());
            cur = p;
        }
        parts.reverse();
        parts
    }

    pub fn remove(&mut self, r: u64) {
        if r == ROOT {
            return;
        }
        let parent = self.nodes.get(&r).and_then(|n| n.parent);
        let mut stack = vec![r];
        while let Some(x) = stack.pop() {
            stack.extend(self.children(x));
            self.nodes.remove(&x);
            self.unreadable.remove(&x);
        }
        if let Some(p) = parent {
            self.touch(p);
        }
    }

    /// Renames and/or moves `r`. Returns false (and does nothing) when not allowed.
    pub fn rename(&mut self, r: u64, new_parent: u64, new_name: &str) -> bool {
        if r == ROOT
            || !self.nodes.get(&new_parent).is_some_and(|p| p.is_dir)
            || self.is_ancestor_or_self(r, new_parent)
            || self.name_taken(new_parent, new_name, Some(r))
        {
            return false;
        }
        let old_parent = self.nodes[&r].parent;
        if let Some(n) = self.nodes.get_mut(&r) {
            n.parent = Some(new_parent);
            new_name.clone_into(&mut n.name);
        }
        if let Some(p) = old_parent {
            self.touch(p);
        }
        self.touch(new_parent);
        true
    }

    pub fn modify(&mut self, r: u64, size: u64) {
        let t = self.tick();
        if let Some(n) = self.nodes.get_mut(&r).filter(|n| !n.is_dir) {
            n.size = size;
            n.modified = t;
        }
    }

    fn meta(r: u64, n: &FakeNode) -> EntryMeta {
        EntryMeta {
            size: n.size,
            alloc_size: Some(n.size.div_ceil(4096) * 4096),
            created: Some(FileTime(1_000)),
            modified: Some(FileTime(n.modified)),
            attrs: if n.is_dir { ATTR_DIRECTORY } else { 0x20 },
            file_ref: Some(r),
        }
    }

    /// Snapshot of the subtree rooted at `from`, as an MFT-based enumerator would produce it.
    pub fn tree(&self, from: u64) -> ScanTree {
        let mut b = ScanTreeBuilder::new(Self::meta(from, &self.nodes[&from]));
        let mut stats = ScanStats::default();
        let mut queue = std::collections::VecDeque::from([(ScanTree::ROOT, from)]);
        while let Some((node, r)) = queue.pop_front() {
            if self.unreadable.contains(&r) {
                b.mark_unreadable(node);
                stats.unreadable_dirs += 1;
                continue;
            }
            let kids = self.children(r);
            let range = b.add_listing(
                node,
                kids.iter().map(|k| {
                    let n = &self.nodes[k];
                    ScanChild {
                        name: n.name.clone(),
                        is_dir: n.is_dir,
                        meta: Self::meta(*k, n),
                    }
                }),
            );
            for (id, k) in range.zip(kids) {
                if self.nodes[&k].is_dir {
                    stats.dirs += 1;
                    queue.push_back((id, k));
                } else {
                    stats.files += 1;
                }
            }
        }
        b.finish(stats)
    }

    /// Builds a random initial tree.
    pub fn populate(&mut self, rng: &mut Rng, creations: usize) {
        for _ in 0..creations {
            let dirs = self.dirs();
            let parent = *rng.pick(&dirs).unwrap_or(&ROOT);
            let name = *rng.pick(NAMES).unwrap_or(&"x");
            let is_dir = rng.below(3) == 0;
            let size = rng.below(10_000) as u64;
            let _ = self.create(parent, name, is_dir, size);
        }
    }

    /// Applies `steps` random changes. Returns the folders whose content changed (old and new
    /// parents), as a change journal would report them.
    pub fn mutate(&mut self, rng: &mut Rng, steps: usize) -> HashSet<u64> {
        let mut dirty = HashSet::new();
        for _ in 0..steps {
            let all: Vec<u64> = self.refs().into_iter().filter(|r| *r != ROOT).collect();
            let dirs = self.dirs();
            let name = *rng.pick(NAMES).unwrap_or(&"x");
            match rng.below(9) {
                0 | 1 => {
                    let parent = *rng.pick(&dirs).unwrap_or(&ROOT);
                    if self
                        .create(parent, name, rng.below(3) == 0, rng.below(10_000) as u64)
                        .is_some()
                    {
                        dirty.insert(parent);
                    }
                }
                2 => {
                    if let Some(&r) = rng.pick(&all) {
                        if let Some(p) = self.nodes[&r].parent {
                            dirty.insert(p);
                        }
                        self.remove(r);
                    }
                }
                3 | 4 => {
                    if let Some(&r) = rng.pick(&all) {
                        let size = if rng.below(2) == 0 {
                            self.nodes[&r].size
                        } else {
                            rng.below(10_000) as u64
                        };
                        if let Some(p) = self.nodes[&r].parent {
                            dirty.insert(p);
                        }
                        self.modify(r, size);
                    }
                }
                5 => {
                    // Rename in place (possibly only the case changes).
                    if let Some(&r) = rng.pick(&all) {
                        let parent = self.nodes[&r].parent.unwrap_or(ROOT);
                        let new_name = if rng.below(3) == 0 {
                            self.nodes[&r].name.to_uppercase()
                        } else {
                            name.to_owned()
                        };
                        if self.rename(r, parent, &new_name) {
                            dirty.insert(parent);
                        }
                    }
                }
                6 => {
                    // Move to another folder.
                    if let (Some(&r), Some(&to)) = (rng.pick(&all), rng.pick(&dirs)) {
                        let from = self.nodes[&r].parent.unwrap_or(ROOT);
                        let keep = self.nodes[&r].name.clone();
                        if self.rename(r, to, &keep) || self.rename(r, to, name) {
                            dirty.insert(from);
                            dirty.insert(to);
                        }
                    }
                }
                7 => {
                    // Replace a file with a folder of the same name.
                    let files: Vec<u64> = all
                        .iter()
                        .copied()
                        .filter(|r| !self.nodes[r].is_dir)
                        .collect();
                    if let Some(&r) = rng.pick(&files) {
                        let n = self.nodes[&r].clone();
                        let parent = n.parent.unwrap_or(ROOT);
                        self.remove(r);
                        let _ = self.create(parent, &n.name, true, 0);
                        dirty.insert(parent);
                    }
                }
                _ => {
                    // Swap the names of two siblings through a temporary name.
                    if let Some(&r) = rng.pick(&all) {
                        let parent = self.nodes[&r].parent.unwrap_or(ROOT);
                        let siblings: Vec<u64> = self
                            .children(parent)
                            .into_iter()
                            .filter(|s| *s != r)
                            .collect();
                        if let Some(&s) = rng.pick(&siblings) {
                            let (nr, ns) =
                                (self.nodes[&r].name.clone(), self.nodes[&s].name.clone());
                            if self.rename(r, parent, "~swap~") {
                                dirty.insert(parent);
                                let _ = self.rename(s, parent, &nr) && self.rename(r, parent, &ns);
                            }
                        }
                    }
                }
            }
        }
        dirty
    }
}

impl Enumerator for FakeFs {
    fn scan(&self, request: &ScanRequest<'_>) -> Result<ScanTree, ScanError> {
        request.cancel.check()?;
        let path = request.relative.join("\\");
        let from = self
            .find(&path)
            .ok_or_else(|| ScanError::RootNotADirectory(request.root.to_path_buf()))?;
        Ok(self.tree(from))
    }
}

/// An enumerator that returns a fixed tree and ignores cancellation, so tests can cancel
/// deterministically during the reconciliation.
pub struct FixedTree(pub ScanTree);

impl Enumerator for FixedTree {
    fn scan(&self, _request: &ScanRequest<'_>) -> Result<ScanTree, ScanError> {
        Ok(self.0.clone())
    }
}
