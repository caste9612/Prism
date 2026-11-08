//! A BK-tree over 64-bit prints with the Hamming distance (F11-AC2): a search within distance
//! `t` visits only the subtrees whose edge distance lies in `[d - t, d + t]`, instead of
//! comparing every pair.

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

/// Groups of prints within `max` of each other, joined transitively (F11-AC1); each group lists
/// positions in `prints`, groups of one are left out.
#[must_use]
pub fn groups(prints: &[u64], max: u32) -> Vec<Vec<usize>> {
    let mut tree = BkTree::new();
    for (i, &p) in prints.iter().enumerate() {
        tree.insert(p, i);
    }
    let mut parent: Vec<usize> = (0..prints.len()).collect();
    for (i, &p) in prints.iter().enumerate() {
        tree.within(p, max, |j| {
            let (a, b) = (find(&mut parent, i), find(&mut parent, j));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        });
    }
    let mut by_root: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for i in 0..prints.len() {
        let r = find(&mut parent, i);
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
