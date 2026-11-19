//! Near prints with the Hamming distance (F11-AC2).
//!
//! [`groups`] puts the prints in groups around a centre: the most repeated print not in a group
//! yet takes every print within the distance not in a group yet, and so on. Joining every pair
//! within the distance, transitively, made chains: on the company PC one group held 1,258,853
//! images of 2.7 million (REVIEW L37). Each print is found through an index of four 16-bit
//! blocks (multi-index hashing): two prints within `t` bits have at least one block within
//! `t / 4` bits. The index is kept twice, by rows of the image (two rows a block) and by columns
//! (two columns a block), and each search uses the one with fewer prints to look at: images with
//! uniform rows (a sky, a white margin) share a block by rows, not by columns. The prints taken
//! leave the index as they are met. The [`BkTree`] answers single searches; over a whole catalog
//! it compared nearly every pair.

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

/// The blocks of a print in the index of [`groups`].
const BLOCKS: u32 = 4;
const BLOCK_BITS: u32 = 64 / BLOCKS;
const VALUES: usize = 1 << BLOCK_BITS;

/// Block `k` of a print by rows (bits `16k..16k+16`: two rows of the image) or by columns
/// (bits `k, k+4, ..., k+60`: two columns).
fn block(print: u64, by_columns: bool, k: u32) -> usize {
    if !by_columns {
        return ((print >> (k * BLOCK_BITS)) & 0xFFFF) as usize;
    }
    let mut x = (print >> k) & 0x1111_1111_1111_1111;
    x = (x | (x >> 3)) & 0x0303_0303_0303_0303;
    x = (x | (x >> 6)) & 0x000F_000F_000F_000F;
    x = (x | (x >> 12)) & 0x0000_00FF_0000_00FF;
    x = (x | (x >> 24)) & 0xFFFF;
    x as usize
}

/// Every 16-bit mask with at most `bits` bits set.
fn masks(bits: u32) -> Vec<usize> {
    (0..VALUES).filter(|m| m.count_ones() <= bits).collect()
}

/// The prints under each value of one block, each with its rank (its place in the order the
/// centres are chosen in), by rank in each bucket, the print kept next to its rank so that a
/// search reads a bucket in one go. A bucket drops, as it is looked at, the prints ranked before
/// the one looking (all in a group already) and the prints found near.
struct Table {
    by_columns: bool,
    k: u32,
    from: Vec<u32>,
    to: Vec<u32>,
    codes: Vec<u64>,
    ranks: Vec<u32>,
}

impl Table {
    /// `codes`: the distinct prints by rank.
    fn new(codes: &[u64], by_columns: bool, k: u32) -> Self {
        let mut start = vec![0u32; VALUES + 1];
        for &c in codes {
            start[block(c, by_columns, k) + 1] += 1;
        }
        for b in 0..VALUES {
            start[b + 1] += start[b];
        }
        let mut next = start.clone();
        let mut in_buckets = vec![0u64; codes.len()];
        let mut ranks = vec![0u32; codes.len()];
        for (rank, &c) in codes.iter().enumerate() {
            let b = block(c, by_columns, k);
            let at = next[b] as usize;
            in_buckets[at] = c;
            ranks[at] = rank as u32;
            next[b] += 1;
        }
        let to = start[1..].to_vec();
        start.truncate(VALUES);
        Self {
            by_columns,
            k,
            from: start,
            to,
            codes: in_buckets,
            ranks,
        }
    }

    /// How many prints the bucket of `code` holds, roughly what a search under it costs.
    fn load(&self, code: u64) -> u64 {
        let b = block(code, self.by_columns, self.k);
        u64::from(self.to[b] - self.from[b])
    }

    /// Puts in the group of the print ranked `rank` (`code`) the prints ranked after it within
    /// `max`, not in a group yet, under the buckets around its block.
    fn take_near(&mut self, rank: u32, code: u64, keys: &[usize], max: u32, centre_of: &mut [u32]) {
        let own = block(code, self.by_columns, self.k);
        for &m in keys {
            let b = own ^ m;
            let end = self.to[b] as usize;
            let mut at = self.from[b] as usize;
            // The prints ranked before are in a group already, for good.
            while at < end && self.ranks[at] <= rank {
                at += 1;
            }
            self.from[b] = at as u32;
            let mut kept = at;
            for i in at..end {
                let c = self.codes[i];
                if distance(code, c) <= max {
                    let r = self.ranks[i] as usize;
                    if centre_of[r] == NONE {
                        centre_of[r] = rank;
                    }
                    continue;
                }
                if kept != i {
                    self.codes[kept] = c;
                    self.ranks[kept] = self.ranks[i];
                }
                kept += 1;
            }
            self.to[b] = kept as u32;
        }
    }
}

const NONE: u32 = u32::MAX;

/// The prints under each value of one block, read only: [`linked`] asks it from several
/// threads whether a print has another near it.
struct Lookup {
    by_columns: bool,
    k: u32,
    start: Vec<u32>,
    codes: Vec<u64>,
}

impl Lookup {
    fn new(codes: &[u64], by_columns: bool, k: u32) -> Self {
        let mut start = vec![0u32; VALUES + 1];
        for &c in codes {
            start[block(c, by_columns, k) + 1] += 1;
        }
        for b in 0..VALUES {
            start[b + 1] += start[b];
        }
        let mut next = start.clone();
        let mut in_buckets = vec![0u64; codes.len()];
        for &c in codes {
            let b = block(c, by_columns, k);
            in_buckets[next[b] as usize] = c;
            next[b] += 1;
        }
        Self {
            by_columns,
            k,
            start,
            codes: in_buckets,
        }
    }

    fn load(&self, code: u64) -> u64 {
        let b = block(code, self.by_columns, self.k);
        u64::from(self.start[b + 1] - self.start[b])
    }

    /// Whether another print is within `max` of `code` under the buckets around its block.
    fn any_near(&self, code: u64, keys: &[usize], max: u32) -> bool {
        let own = block(code, self.by_columns, self.k);
        keys.iter().any(|&m| {
            let b = own ^ m;
            self.codes[self.start[b] as usize..self.start[b + 1] as usize]
                .iter()
                .any(|&c| c != code && distance(code, c) <= max)
        })
    }
}

/// Prints looked at by a thread at a time.
const CHUNK: usize = 4096;

/// Whether each of `codes` (distinct prints) has another within `max`. Most prints of a catalog
/// have none, and finding it out takes dozens of reads scattered in memory for each: the
/// threads share it. The others alone go on to the groups.
fn linked(codes: &[u64], keys: &[usize], max: u32) -> Vec<bool> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get() / 2)
        .clamp(1, 8);
    let tables: Vec<Lookup> = std::thread::scope(|s| {
        let built: Vec<_> = [false, true]
            .into_iter()
            .flat_map(|by_columns| (0..BLOCKS).map(move |k| (by_columns, k)))
            .map(|(by_columns, k)| s.spawn(move || Lookup::new(codes, by_columns, k)))
            .collect();
        built
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    let half = BLOCKS as usize;
    let mut out = vec![false; codes.len()];
    let work = std::sync::Mutex::new(out.chunks_mut(CHUNK).enumerate());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let next = work
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .next();
                    let Some((chunk, found)) = next else { break };
                    for (i, f) in found.iter_mut().enumerate() {
                        let code = codes[chunk * CHUNK + i];
                        let load = |ts: &[Lookup]| ts.iter().map(|t| t.load(code)).sum::<u64>();
                        let chosen = if load(&tables[half..]) < load(&tables[..half]) {
                            &tables[half..]
                        } else {
                            &tables[..half]
                        };
                        *f = chosen.iter().any(|t| t.any_near(code, keys, max));
                    }
                }
            });
        }
    });
    out
}

/// For each of `codes` (distinct prints, in the order the centres are chosen in, each with
/// another within `max`), the rank of the centre of its group: each print not in a group yet
/// takes those after it within `max`.
fn leaders(codes: &[u64], keys: &[usize], max: u32) -> Vec<u32> {
    let mut tables: Vec<Table> = [false, true]
        .into_iter()
        .flat_map(|by_columns| (0..BLOCKS).map(move |k| (by_columns, k)))
        .map(|(by_columns, k)| Table::new(codes, by_columns, k))
        .collect();
    let half = BLOCKS as usize;
    let mut centre_of = vec![NONE; codes.len()];
    for (rank, &code) in codes.iter().enumerate() {
        if centre_of[rank] != NONE {
            continue;
        }
        let rank = rank as u32;
        centre_of[rank as usize] = rank;
        // The index (by rows or by columns) with fewer prints under the blocks of this one.
        let load = |ts: &[Table]| ts.iter().map(|t| t.load(code)).sum::<u64>();
        let chosen = if load(&tables[half..]) < load(&tables[..half]) {
            &mut tables[half..]
        } else {
            &mut tables[..half]
        };
        for t in chosen {
            t.take_near(rank, code, keys, max, &mut centre_of);
        }
    }
    centre_of
}

/// How a grouping went, for the log (REVIEW L38).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupingStats {
    /// Distinct prints.
    pub distinct: usize,
    /// Distinct prints with another within the distance.
    pub linked: usize,
    /// Time to find them (several threads), then to make the groups (one).
    pub linked_ms: u128,
    pub grouping_ms: u128,
}

/// For each of `codes` (distinct prints, in the order the centres are chosen in), the rank of
/// the centre of its group. The prints without another within `max` are their own centre and
/// stay out of the search for the groups, which goes the same without them.
fn centres(codes: &[u64], max: u32, stats: &mut GroupingStats) -> Vec<u32> {
    let mut centre_of: Vec<u32> = (0..codes.len() as u32).collect();
    stats.distinct = codes.len();
    if max == 0 {
        return centre_of;
    }
    let keys = masks(max / BLOCKS);
    let started = std::time::Instant::now();
    let near = linked(codes, &keys, max);
    let ranks: Vec<u32> = (0..codes.len() as u32)
        .filter(|&r| near[r as usize])
        .collect();
    drop(near);
    stats.linked = ranks.len();
    stats.linked_ms = started.elapsed().as_millis();
    let started = std::time::Instant::now();
    let some: Vec<u64> = ranks.iter().map(|&r| codes[r as usize]).collect();
    for (i, c) in leaders(&some, &keys, max).into_iter().enumerate() {
        centre_of[ranks[i] as usize] = ranks[c as usize];
    }
    stats.grouping_ms = started.elapsed().as_millis();
    centre_of
}

/// A group of near prints: positions in the prints given to [`groups`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// An item with the print at the centre of the group: every other item is within the
    /// distance of it.
    pub centre: usize,
    /// Every item of the group, the centre among them, in order.
    pub items: Vec<usize>,
}

/// Groups of prints within `max` of a centre (F11-AC1, REVIEW L37): the most repeated print
/// not in a group yet (the lowest print among equals) takes every print within `max` not in a
/// group yet. Groups of one are left out; the groups come in the order of their first item.
#[must_use]
pub fn groups(prints: &[u64], max: u32) -> Vec<Group> {
    groups_with_stats(prints, max).0
}

/// [`groups`], with how the grouping went.
#[must_use]
pub fn groups_with_stats(prints: &[u64], max: u32) -> (Vec<Group>, GroupingStats) {
    // Identical prints are one node.
    let mut node_of: std::collections::HashMap<u64, u32> =
        std::collections::HashMap::with_capacity(prints.len());
    let mut codes: Vec<u64> = Vec::new();
    let mut count: Vec<u32> = Vec::new();
    let mut nodes: Vec<u32> = prints
        .iter()
        .map(|&p| {
            let n = *node_of.entry(p).or_insert_with(|| {
                codes.push(p);
                count.push(0);
                (codes.len() - 1) as u32
            });
            count[n as usize] += 1;
            n
        })
        .collect();
    drop(node_of);
    // The nodes by rank: the most repeated first.
    let mut order: Vec<u32> = (0..codes.len() as u32).collect();
    order.sort_unstable_by_key(|&n| (std::cmp::Reverse(count[n as usize]), codes[n as usize]));
    drop(count);
    let mut rank = vec![0u32; codes.len()];
    for (r, &n) in order.iter().enumerate() {
        rank[n as usize] = r as u32;
    }
    let ranked: Vec<u64> = order.iter().map(|&n| codes[n as usize]).collect();
    drop((order, codes));
    for n in &mut nodes {
        *n = rank[*n as usize];
    }
    drop(rank);
    let mut stats = GroupingStats::default();
    let centre_of = centres(&ranked, max, &mut stats);
    let mut size = vec![0u32; ranked.len()];
    for &r in &nodes {
        size[centre_of[r as usize] as usize] += 1;
    }
    let mut slot = vec![NONE; ranked.len()];
    let mut out: Vec<Group> = Vec::new();
    for (i, &r) in nodes.iter().enumerate() {
        let c = centre_of[r as usize] as usize;
        if size[c] < 2 {
            continue;
        }
        if slot[c] == NONE {
            slot[c] = out.len() as u32;
            out.push(Group {
                centre: usize::MAX,
                items: Vec::with_capacity(size[c] as usize),
            });
        }
        let g = &mut out[slot[c] as usize];
        if r as usize == c && g.centre == usize::MAX {
            g.centre = i;
        }
        g.items.push(i);
    }
    (out, stats)
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
    fn blocks_by_columns_take_every_fourth_bit() {
        let p = 0x8000_0000_0000_0001u64;
        assert_eq!(block(p, true, 0), 1);
        assert_eq!(block(p, true, 3), 1 << 15);
        assert_eq!(block(p, false, 0), 1);
        assert_eq!(block(p, false, 3), 1 << 15);
        // Every bit lands in exactly one block, once.
        for b in 0..64 {
            let p = 1u64 << b;
            let set: u32 = (0..BLOCKS).map(|k| block(p, true, k).count_ones()).sum();
            assert_eq!(set, 1, "bit {b}");
            assert_eq!(block(p, true, b % 4), 1 << (b / 4), "bit {b}");
        }
    }

    /// The groups a comparison of every pair gives, with the same centres in the same order.
    fn groups_by_every_pair(prints: &[u64], max: u32) -> Vec<Group> {
        let mut codes: Vec<u64> = Vec::new();
        for &p in prints {
            if !codes.contains(&p) {
                codes.push(p);
            }
        }
        let count = |c: u64| prints.iter().filter(|&&p| p == c).count();
        let mut order: Vec<u64> = codes.clone();
        order.sort_by_key(|&c| (std::cmp::Reverse(count(c)), c));
        let mut centre_of: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        for &c in &order {
            if centre_of.contains_key(&c) {
                continue;
            }
            for &o in &codes {
                if !centre_of.contains_key(&o) && distance(c, o) <= max {
                    centre_of.insert(o, c);
                }
            }
        }
        let mut out: Vec<Group> = Vec::new();
        let mut seen: Vec<u64> = Vec::new();
        for &p in prints {
            let c = centre_of[&p];
            if seen.contains(&c) {
                continue;
            }
            seen.push(c);
            let items: Vec<usize> = (0..prints.len())
                .filter(|&j| centre_of[&prints[j]] == c)
                .collect();
            if items.len() > 1 {
                let centre = *items.iter().find(|&&j| prints[j] == c).unwrap();
                out.push(Group { centre, items });
            }
        }
        out
    }

    // REVIEW L34, L37: the index of the blocks finds the groups a comparison of every pair
    // finds, at every distance the screen offers and beyond, with repeated prints, near copies
    // spread over every block and prints that share whole blocks without being near.
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
        // Uniform rows (a zero block by rows) and uniform columns (a zero block by columns).
        for (i, &p) in prints(200, 0x1234_5678_9ABC_DEF1).iter().enumerate() {
            data.push(p & !0xFFFF);
            data.push(p & !(0x1111_1111_1111_1111 << (i % 4)));
        }
        // Repeated prints, near-blank images among them.
        data.extend([
            0,
            0,
            0,
            data[5],
            data[5],
            1,
            1 << 63,
            3,
            7,
            0x8000_0000_0000_0080,
        ]);
        for max in [0, 1, 3, 4, 6, 8, 12, 16, 20] {
            assert_eq!(
                groups(&data, max),
                groups_by_every_pair(&data, max),
                "max {max}"
            );
        }
    }

    // REVIEW L37: no chains. a, b, c are 2 bits apart one after the other, a and c 4: at
    // distance 2 the most repeated takes its neighbours, the last one stays alone.
    #[test]
    fn groups_gather_around_a_centre_without_chains() {
        let a = 0xF0F0_F0F0_F0F0_F0F0u64;
        let (b, c) = (a ^ 0b11, a ^ 0b1111);
        let data = [a, b, c, !a, a, 0x1234_5678_9ABC_DEF0];
        assert_eq!(
            groups(&data, 2),
            [Group {
                centre: 0,
                items: vec![0, 1, 4]
            }]
        );
        // The most repeated print is the centre, whatever comes first.
        let data = [b, c, c, a];
        assert_eq!(
            groups(&data, 2),
            [Group {
                centre: 1,
                items: vec![0, 1, 2]
            }]
        );
        assert_eq!(
            groups(&[a, b, a], 0),
            [Group {
                centre: 0,
                items: vec![0, 2]
            }]
        );
        assert!(groups(&[0, u64::MAX], 3).is_empty());
    }

    /// A print near `base`: `flips` bits changed at positions from `seed`.
    fn near(base: u64, flips: u32, seed: &mut u64) -> u64 {
        let mut p = base;
        for _ in 0..flips {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            p ^= 1 << (*seed % 64);
        }
        p
    }

    // REVIEW L37: a catalog like the company one, measured. `cargo test -p prism-core --release
    // -- --ignored grouping_a_large_catalog --nocapture`; PRISM_PRINTS_FILE=<file of u64 LE>
    // adds real prints.
    #[test]
    #[ignore = "measurement"]
    fn grouping_a_large_catalog() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut data = prints(1_200_000, 0x2545_F491_4F6C_DD1D);
        // Blank slides and their noise: few centres, a few bits apart.
        for i in 0..600_000u64 {
            let centre = [
                0xF0F0_F0F0_F0F0_F0F0u64,
                0x8080_8080_8080_8080,
                0x0F0F_0F0F_0F0F_0F0F,
            ][(i % 3) as usize];
            data.push(near(centre, 1 + (i % 5) as u32, &mut seed));
        }
        // Near-blank images: a few bits anywhere.
        for i in 0..300_000u64 {
            data.push(near(0, 1 + (i % 12) as u32, &mut seed));
        }
        // Uniform top rows, uniform side columns: whole blocks shared, the rest apart.
        for (i, p) in prints(600_000, 0x1234_5678_9ABC_DEF1)
            .into_iter()
            .enumerate()
        {
            data.push(if i % 2 == 0 {
                p & !0xFFFF_0000_0000_0000
            } else {
                p & !0x1111_1111_1111_1111
            });
        }
        if let Ok(path) = std::env::var("PRISM_PRINTS_FILE") {
            let bytes = std::fs::read(path).unwrap();
            data.extend(
                bytes
                    .chunks_exact(8)
                    .map(|c| u64::from_le_bytes(c.try_into().unwrap())),
            );
        }
        for (name, part) in [
            ("random", &data[..1_200_000]),
            ("slides", &data[1_200_000..1_800_000]),
            ("near blank", &data[1_800_000..2_100_000]),
            ("uniform rows/columns", &data[2_100_000..2_700_000]),
            ("real", &data[2_700_000..]),
        ] {
            let started = std::time::Instant::now();
            let found = groups(part, 6);
            eprintln!(
                "{name}: {} prints, {} groups, largest {}, {} ms",
                part.len(),
                found.len(),
                found.iter().map(|g| g.items.len()).max().unwrap_or(0),
                started.elapsed().as_millis()
            );
        }
        for max in [6, 10, 3, 0] {
            let started = std::time::Instant::now();
            let found = groups(&data, max);
            eprintln!(
                "max {max}: {} prints, {} groups, largest {}, {} ms",
                data.len(),
                found.len(),
                found.iter().map(|g| g.items.len()).max().unwrap_or(0),
                started.elapsed().as_millis()
            );
        }
    }

    // REVIEW L38: the company catalog has three quarters of its images in groups (series of
    // near tiles), not a few large clusters; measured phase by phase.
    #[test]
    #[ignore = "measurement"]
    fn grouping_series_of_near_pictures() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut data = prints(700_000, 0x2545_F491_4F6C_DD1D);
        // 400,000 series of five near pictures, each a few bits from the one before.
        for centre in prints(400_000, 0x1234_5678_9ABC_DEF1) {
            let mut p = centre;
            for _ in 0..5 {
                data.push(p);
                p = near(p, 2, &mut seed);
            }
        }
        // One crowd: 30,000 pictures within a few bits of one print.
        for i in 0..30_000u32 {
            data.push(near(0xF0F0_F0F0_F0F0_F0F0, 1 + i % 4, &mut seed));
        }
        let started = std::time::Instant::now();
        let found = groups(&data, 6);
        eprintln!(
            "series: {} prints, {} groups, largest {}, in groups {}, {} ms",
            data.len(),
            found.len(),
            found.iter().map(|g| g.items.len()).max().unwrap_or(0),
            found.iter().map(|g| g.items.len()).sum::<usize>(),
            started.elapsed().as_millis()
        );
        let mut codes = data.clone();
        codes.sort_unstable();
        codes.dedup();
        let keys = masks(6 / BLOCKS);
        let started = std::time::Instant::now();
        let near_ones = linked(&codes, &keys, 6);
        let some: Vec<u64> = codes
            .iter()
            .zip(&near_ones)
            .filter(|(_, n)| **n)
            .map(|(c, _)| *c)
            .collect();
        eprintln!(
            "linked: {} of {} distinct, {} ms",
            some.len(),
            codes.len(),
            started.elapsed().as_millis()
        );
        let started = std::time::Instant::now();
        let centre_of = leaders(&some, &keys, 6);
        eprintln!(
            "leaders: {} centres, {} ms",
            centre_of
                .iter()
                .enumerate()
                .filter(|(i, c)| **c as usize == *i)
                .count(),
            started.elapsed().as_millis()
        );
    }
}
