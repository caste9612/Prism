//! Near prints with the Hamming distance (F11-AC2).
//!
//! [`groups`] joins the prints within a distance of each other with an index of their four
//! 16-bit blocks (multi-index hashing): two prints within `t` bits have at least one block within
//! `t / 4` bits, so each print is compared only with the prints found under its own blocks and
//! the blocks a few bits away. Identical prints are one node: on a real catalog 10,022 blank
//! images shared the print 0, and comparing them with each other took 100 million steps (REVIEW
//! L34). The [`BkTree`] answers single searches; over a whole catalog it compared nearly every
//! pair (the time grew fourfold each time the prints doubled).

/// Hamming distance between two prints.
#[must_use]
pub fn distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

struct Node {
    print: u64,
    /// Items with exactly this print.
    items: Vec<usize>,
    /// Children by distance from this node (at most 65 different distances).
    children: Vec<(u32, usize)>,
}

#[derive(Default)]
pub struct BkTree {
    nodes: Vec<Node>,
}

impl BkTree {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds item `item` with print `print`.
    pub fn insert(&mut self, print: u64, item: usize) {
        if self.nodes.is_empty() {
            self.nodes.push(Node {
                print,
                items: vec![item],
                children: Vec::new(),
            });
            return;
        }
        let mut at = 0;
        loop {
            let d = distance(self.nodes[at].print, print);
            if d == 0 {
                self.nodes[at].items.push(item);
                return;
            }
            match self.nodes[at].children.iter().find(|(cd, _)| *cd == d) {
                Some(&(_, child)) => at = child,
                None => {
                    let new = self.nodes.len();
                    self.nodes.push(Node {
                        print,
                        items: vec![item],
                        children: Vec::new(),
                    });
                    self.nodes[at].children.push((d, new));
                    return;
                }
            }
        }
    }

    /// Calls `found` for every item within `max` of `print`.
    pub fn within(&self, print: u64, max: u32, mut found: impl FnMut(usize)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0usize];
        while let Some(at) = stack.pop() {
            let node = &self.nodes[at];
            let d = distance(node.print, print);
            if d <= max {
                for &i in &node.items {
                    found(i);
                }
            }
            let (lo, hi) = (d.saturating_sub(max), d + max);
            stack.extend(
                node.children
                    .iter()
                    .filter(|(cd, _)| (lo..=hi).contains(cd))
                    .map(|&(_, c)| c),
            );
        }
    }
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// The blocks of a print in the index of [`groups`].
const BLOCKS: u32 = 4;
const BLOCK_BITS: u32 = 64 / BLOCKS;

fn block(print: u64, k: u32) -> usize {
    ((print >> (k * BLOCK_BITS)) & 0xFFFF) as usize
}

/// Every 16-bit mask with at most `bits` bits set.
fn masks(bits: u32) -> Vec<usize> {
    (0..1usize << BLOCK_BITS)
        .filter(|m| m.count_ones() <= bits)
        .collect()
}

/// For each of `codes` (distinct prints), the first of the codes it is joined with, directly or
/// through others, within `max` bits.
fn join_near(codes: &[u64], max: u32) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..codes.len()).collect();
    if max == 0 || codes.len() < 2 {
        return parent;
    }
    let keys = masks(max / BLOCKS);
    let buckets = 1usize << BLOCK_BITS;
    for k in 0..BLOCKS {
        // The codes by the value of block `k`, in their order (the bucket of a value lists its
        // codes from the lowest position).
        let mut start = vec![0usize; buckets + 1];
        for &c in codes {
            start[block(c, k) + 1] += 1;
        }
        for b in 0..buckets {
            start[b + 1] += start[b];
        }
        let mut next = start.clone();
        let mut by_block = vec![0usize; codes.len()];
        for (i, &c) in codes.iter().enumerate() {
            let b = block(c, k);
            by_block[next[b]] = i;
            next[b] += 1;
        }
        for (i, &c) in codes.iter().enumerate() {
            let own = block(c, k);
            for &m in &keys {
                let bucket = &by_block[start[own ^ m]..start[(own ^ m) + 1]];
                // Each pair once: the codes after this one.
                let after = bucket.partition_point(|&j| j <= i);
                for &j in &bucket[after..] {
                    if distance(c, codes[j]) <= max {
                        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                        if a != b {
                            parent[a.max(b)] = a.min(b);
                        }
                    }
                }
            }
        }
    }
    parent
}

/// Groups of prints within `max` of each other, joined transitively (F11-AC1); each group lists
/// positions in `prints`, groups of one are left out.
#[must_use]
pub fn groups(prints: &[u64], max: u32) -> Vec<Vec<usize>> {
    // Identical prints are one node.
    let mut node_of: std::collections::HashMap<u64, usize> =
        std::collections::HashMap::with_capacity(prints.len());
    let mut codes: Vec<u64> = Vec::new();
    let nodes: Vec<usize> = prints
        .iter()
        .map(|&p| {
            *node_of.entry(p).or_insert_with(|| {
                codes.push(p);
                codes.len() - 1
            })
        })
        .collect();
    drop(node_of);
    let mut parent = join_near(&codes, max);
    let mut by_root: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, &n) in nodes.iter().enumerate() {
        let r = find(&mut parent, n);
        by_root.entry(r).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = by_root.into_values().filter(|g| g.len() > 1).collect();
    out.sort_by_key(|g| g[0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic pseudo-random sequence (xorshift).
    fn prints(n: usize, mut seed: u64) -> Vec<u64> {
        (0..n)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed
            })
            .collect()
    }

    // F11-AC2: the tree finds exactly what a comparison of every pair finds.
    #[test]
    fn finds_the_same_neighbours_as_brute_force() {
        let mut data = prints(3000, 0x9E37_79B9_7F4A_7C15);
        // Near copies of some prints.
        for i in 0..200 {
            let base = data[i * 7];
            data.push(base ^ (1 << (i % 64)) ^ (1 << ((i * 3) % 64)));
        }
        let mut tree = BkTree::new();
        for (i, &p) in data.iter().enumerate() {
            tree.insert(p, i);
        }
        for &q in data.iter().step_by(37) {
            for max in [0, 4, 10] {
                let mut fast = Vec::new();
                tree.within(q, max, |i| fast.push(i));
                fast.sort_unstable();
                let slow: Vec<usize> = (0..data.len())
                    .filter(|&i| distance(data[i], q) <= max)
                    .collect();
                assert_eq!(fast, slow, "max {max}");
            }
        }
    }

    /// The groups a comparison of every pair gives.
    fn groups_by_every_pair(prints: &[u64], max: u32) -> Vec<Vec<usize>> {
        let mut parent: Vec<usize> = (0..prints.len()).collect();
        for i in 0..prints.len() {
            for j in i + 1..prints.len() {
                if distance(prints[i], prints[j]) <= max {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    if a != b {
                        parent[a.max(b)] = a.min(b);
                    }
                }
            }
        }
        let mut by_root: std::collections::BTreeMap<usize, Vec<usize>> =
            std::collections::BTreeMap::new();
        for i in 0..prints.len() {
            let r = find(&mut parent, i);
            by_root.entry(r).or_default().push(i);
        }
        let mut out: Vec<Vec<usize>> = by_root.into_values().filter(|g| g.len() > 1).collect();
        out.sort_by_key(|g| g[0]);
        out
    }

    // REVIEW L34: the index of the blocks finds the groups a comparison of every pair finds, at
    // every distance the screen offers and beyond, with repeated prints and near copies spread
    // over every block.
    #[test]
    fn groups_are_those_of_every_pair() {
        let mut data = prints(1500, 0x2545_F491_4F6C_DD1D);
        for i in 0..300 {
            let base = data[(i * 11) % 1500];
            let flips = [
                i % 64,
                (i * 7 + 3) % 64,
                (i * 13 + 5) % 64,
                (i * 29 + 1) % 64,
            ];
            let near = flips[..1 + i % 4]
                .iter()
                .fold(base, |p, &b| p ^ (1u64 << b));
            data.push(near);
        }
        // Repeated prints, blank images among them.
        data.extend([0, 0, 0, data[5], data[5], 1, 1 << 63]);
        for max in [0, 1, 3, 4, 6, 8, 12, 16, 20] {
            assert_eq!(
                groups(&data, max),
                groups_by_every_pair(&data, max),
                "max {max}"
            );
        }
    }

    #[test]
    fn groups_join_near_prints_transitively() {
        let a = 0xF0F0_F0F0_F0F0_F0F0u64;
        let data = [a, a ^ 0b11, a ^ 0b1111, !a, a, 0x1234_5678_9ABC_DEF0];
        // a and a^0b11 differ by 2, a^0b11 and a^0b1111 by 2: one group at max 2.
        assert_eq!(groups(&data, 2), [vec![0, 1, 2, 4]]);
        assert_eq!(groups(&data, 0), [vec![0, 4]]);
        assert!(groups(&[0, u64::MAX], 3).is_empty());
    }
}
