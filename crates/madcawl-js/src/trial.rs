//! Trial deletion over a side graph.
//!
//! The collector copies each vertex's count and its reported edges
//! into this graph and decides there, leaving the objects' headers
//! alone, so the graph can be built in batches while script runs in
//! between. A vertex whose count is larger than the number of edges
//! the graph has into it is held from outside; everything reachable
//! from such a vertex is alive; the rest is garbage.
//!
//! The same decision runs twice in a collection: once over the graph
//! built in batches, and once more, in one step, over the vertices it
//! found to be garbage, with their counts and edges read afresh. A set
//! whose every count is explained by edges from inside the set cannot
//! be reached from outside, whatever changed between batches.

use std::collections::HashMap;
use std::hash::Hash;

pub struct SideGraph<K> {
    keys: Vec<K>,
    index: HashMap<K, u32>,
    counts: Vec<u32>,
    /// Known to be alive whatever the counts say.
    pinned: Vec<bool>,
    edges: Vec<(u32, K)>,
}

/// More edges were reported into `key` than its count: some `traverse`
/// reported an edge that is not counted, which would free a live
/// object.
#[derive(Debug, PartialEq, Eq)]
pub struct Overreported<K> {
    pub key: K,
    pub count: u32,
    pub reported: u32,
}

impl<K: Copy + Eq + Hash> SideGraph<K> {
    pub fn new() -> Self {
        SideGraph {
            keys: Vec::new(),
            index: HashMap::new(),
            counts: Vec::new(),
            pinned: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Add `key` with `count`, unless it is already there; its index
    /// and whether it is new.
    pub fn vertex(&mut self, key: K, count: u32) -> (u32, bool) {
        if let Some(&i) = self.index.get(&key) {
            return (i, false);
        }
        let i = u32::try_from(self.keys.len()).expect("under 2^32 vertices");
        self.keys.push(key);
        self.index.insert(key, i);
        self.counts.push(count);
        self.pinned.push(false);
        (i, true)
    }

    pub fn contains(&self, key: K) -> bool {
        self.index.contains_key(&key)
    }

    /// An edge from vertex `from` to `to`; an edge to a key that never
    /// becomes a vertex is held by something outside the graph and
    /// does not count.
    pub fn edge(&mut self, from: u32, to: K) {
        self.edges.push((from, to));
    }

    /// Treat `key` as held from outside.
    pub fn pin(&mut self, key: K) {
        if let Some(&i) = self.index.get(&key) {
            self.pinned[i as usize] = true;
        }
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn keys(&self) -> &[K] {
        &self.keys
    }

    /// The vertices no one outside the graph can reach, for a graph
    /// whose counts and edges were read in one step: more edges into a
    /// vertex than its count is a broken `traverse`.
    pub fn garbage(&self) -> Result<Vec<K>, Overreported<K>> {
        self.decide(true)
    }

    /// The same for a graph built in batches, where a count read in one
    /// batch and edges read in a later one may disagree (a node moved
    /// between parents meanwhile): such a vertex is counted alive.
    pub fn garbage_batched(&self) -> Vec<K> {
        match self.decide(false) {
            Ok(g) => g,
            Err(_) => unreachable!("only the strict decision fails"),
        }
    }

    fn decide(&self, strict: bool) -> Result<Vec<K>, Overreported<K>> {
        let n = self.keys.len();
        let resolved: Vec<(u32, u32)> = self
            .edges
            .iter()
            .filter_map(|&(from, to)| self.index.get(&to).map(|&t| (from, t)))
            .collect();
        let mut internal = vec![0u32; n];
        let mut start = vec![0u32; n + 1];
        for &(from, to) in &resolved {
            internal[to as usize] += 1;
            start[from as usize + 1] += 1;
        }
        for i in 0..n {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut out = vec![0u32; resolved.len()];
        for &(from, to) in &resolved {
            out[fill[from as usize] as usize] = to;
            fill[from as usize] += 1;
        }
        let mut live = vec![false; n];
        let mut work = Vec::new();
        for v in 0..n {
            if internal[v] > self.counts[v] && strict {
                return Err(Overreported {
                    key: self.keys[v],
                    count: self.counts[v],
                    reported: internal[v],
                });
            }
            if self.pinned[v] || self.counts[v] != internal[v] {
                live[v] = true;
                work.push(v as u32);
            }
        }
        while let Some(v) = work.pop() {
            for &t in &out[start[v as usize] as usize..start[v as usize + 1] as usize] {
                if !live[t as usize] {
                    live[t as usize] = true;
                    work.push(t);
                }
            }
        }
        Ok((0..n)
            .filter(|&v| !live[v])
            .map(|v| self.keys[v])
            .collect())
    }
}

impl<K: Copy + Eq + Hash> Default for SideGraph<K> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(counts: &[u32], edges: &[(u32, u32)]) -> SideGraph<u32> {
        let mut g = SideGraph::new();
        for (k, &c) in counts.iter().enumerate() {
            g.vertex(k as u32, c);
        }
        for &(a, b) in edges {
            g.edge(a, b);
        }
        g
    }

    #[test]
    fn a_closed_cycle_is_garbage() {
        let g = graph(&[1, 1, 1], &[(0, 1), (1, 2), (2, 0)]);
        assert_eq!(g.garbage().unwrap(), vec![0, 1, 2]);
    }

    #[test]
    fn one_outside_count_keeps_everything_it_reaches() {
        // 1 is also held from outside; 3 hangs off the cycle, 4 is a
        // separate dead pair
        let g = graph(
            &[1, 2, 1, 1, 1, 1],
            &[(0, 1), (1, 2), (2, 0), (2, 3), (4, 5), (5, 4)],
        );
        assert_eq!(g.garbage().unwrap(), vec![4, 5]);
    }

    #[test]
    fn an_edge_out_of_the_graph_does_not_count_and_a_pin_keeps() {
        let mut g = graph(&[1, 1], &[(0, 1), (1, 0), (1, 99)]);
        assert_eq!(g.garbage().unwrap(), vec![0, 1]);
        g.pin(1);
        assert_eq!(g.garbage().unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn an_edge_reported_twice_is_caught() {
        let g = graph(&[1, 1], &[(0, 1), (0, 1), (1, 0)]);
        assert_eq!(g.garbage_batched(), Vec::<u32>::new(), "alive when built in batches");
        assert_eq!(
            g.garbage(),
            Err(Overreported {
                key: 1,
                count: 1,
                reported: 2
            })
        );
    }
}
