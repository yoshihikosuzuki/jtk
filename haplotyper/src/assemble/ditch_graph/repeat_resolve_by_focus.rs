use std::collections::BTreeMap;
use std::collections::HashMap;

type NodeWithTraverseInfo = (usize, usize, usize, NodeIndex, Position);
const ERROR_PROB: f64 = 0.05;
use super::super::AssembleConfig;
use super::DitchGraph;
use super::DitchNode;
use super::Node;
use super::NodeIndex;
use super::Position;
use definitions::*;
/// A focus from a node of a ditch graph.
/// Here, we tag a node with its position, i.e., Position.
#[derive(Debug, Clone)]
struct Focus {
    from: NodeIndex,
    from_node: Node,
    from_position: Position,
    to: NodeIndex,
    to_node: Node,
    to_position: Position,
    dist: usize,
    // Log Likelihood ratio between the alt hypothesis/null hypothesis.
    log_likelihood_ratio: f64,
    counts: Vec<usize>,
    path: Vec<(NodeIndex, Position)>,
}

impl std::fmt::Display for Focus {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let &Self {
            from_node,
            from_position,
            to_node,
            to_position,
            dist,
            log_likelihood_ratio,
            ref counts,
            ..
        } = self;
        let (fu, fc) = from_node;
        let (tu, tc) = to_node;
        write!(
            f,
            "{fu},{fc},{from_position},{tu},{tc},{to_position},{dist},{:.2},{:?}",
            log_likelihood_ratio, counts
        )
    }
}

impl Focus {
    /// LogLilihood ratio
    fn llr(&self) -> f64 {
        self.log_likelihood_ratio
    }

    fn with_backpath(
        (from, from_node, from_position): (NodeIndex, Node, Position),
        (to, to_node, to_position): (NodeIndex, Node, Position),
        dist: usize,
        lk: f64,
        counts: Vec<usize>,
        path: Vec<(NodeIndex, Position)>,
    ) -> Self {
        Self {
            from,
            from_node,
            from_position,
            to,
            to_node,
            to_position,
            dist,
            log_likelihood_ratio: lk,
            counts,
            path,
        }
    }
}

use std::collections::HashSet;
impl<'b, 'a: 'b> DitchGraph<'a> {
    fn survey_foci(&'b mut self, foci: &[Focus]) -> usize {
        let mut solved = 0;
        let mut affected_nodes: HashSet<NodeIndex> = HashSet::new();
        for focus in foci {
            if focus.path.iter().any(|n| affected_nodes.contains(&n.0)) {
                continue;
            }
            // Check if the targets are present in the graph.
            let from_copy_num = self.node(focus.from).and_then(|n| n.copy_number);
            let to_copy_num = self.node(focus.to).and_then(|node| node.copy_number);
            if !matches!(from_copy_num, Some(1)) || !matches!(to_copy_num, Some(1)) {
                continue;
            }
            // Check if this is branching.
            let (key, pos) = (focus.from, focus.from_position);
            if self.count_edges(key, pos) != 1 {
                continue;
            }
            // Check this branch has two-in-degree.
            let mut edges = self.edges_from(key, pos);
            let edge = edges.pop().unwrap();
            let sibs = self.count_edges(edge.to, edge.to_position);
            if sibs <= 1 {
                continue;
            }
            debug!("FOCUS\tTRY\t{}", focus);
            solved += self.survey_focus(focus, &mut affected_nodes).is_some() as usize;
        }
        solved
    }

    fn is_path_branching(&self, focus: &Focus) -> bool {
        let len = focus.path.len();
        let mut is_branching = false;
        is_branching |= 1 < self.count_edges(focus.from, focus.from_position);
        let mut check_both = focus.path.iter().take(len.saturating_sub(1));
        is_branching |= check_both.any(|&(node, _)| {
            let edges = self.node(node).unwrap().edges.iter();
            let (head, tail) = edges.fold((0, 0), |(h, t), e| match e.from_position {
                Position::Head => (h + 1, t),
                Position::Tail => (h, t + 1),
            });
            1 < head || 1 < tail
        });
        if let Some(&(node, position)) = focus.path.last() {
            is_branching |= 1 < self.count_edges(node, position);
        }
        is_branching
    }
    // Return nodes newly allocated.
    fn duplicate_along(&'b mut self, focus: &Focus) -> Vec<NodeIndex> {
        let path_spaning = &focus.path;
        let (mut c_index, mut c_pos) = (focus.from, focus.from_position);
        let mut prev_id = focus.from;
        let mut newly_allocated = vec![];
        for (i, &(to_id, to_pos)) in path_spaning.iter().enumerate() {
            // Duplicate if nessesary.
            let new_idx = match i + 1 == path_spaning.len() {
                true => to_id,
                false => {
                    let occ = self
                        .node_mut(to_id)
                        .map(|n| n.decrement_copy_number())
                        .unwrap_or(0);
                    let new_idx = self.duplicate(to_id);
                    if let Some(n) = self.node_mut(new_idx) {
                        n.occ = occ;
                        n.copy_number = Some(1);
                    }
                    newly_allocated.push(new_idx);
                    new_idx
                }
            };
            let edge_occ = self.decrement_edge_copy_number((c_index, c_pos), (to_id, to_pos));
            let mut edge = self
                .node(c_index)
                .unwrap()
                .edges
                .iter()
                .find(|e| (e.from_position, e.to, e.to_position) == (c_pos, to_id, to_pos))
                .unwrap()
                .clone();
            edge.occ = edge_occ;
            edge.copy_number = Some(1);
            edge.from = prev_id;
            edge.to = new_idx;
            self.add_edge(edge);
            c_index = to_id;
            c_pos = !to_pos;
            prev_id = new_idx;
        }
        newly_allocated
    }

    fn remove_used_edges(&'b mut self, focus: &Focus) {
        let (mut prev, mut prev_pos) = (focus.from, focus.from_position);
        for &(node, pos) in focus.path.iter() {
            let prev_node = self.node(prev).unwrap();
            let copy_num = prev_node
                .edges
                .iter()
                .find(|e| e.from_position == prev_pos && e.to == node && e.to_position == pos)
                .and_then(|e| e.copy_number);
            if copy_num == Some(0) {
                self.remove_edge_between((prev, prev_pos), (node, pos));
            }
            prev = node;
            prev_pos = !pos;
        }
    }
    fn remove_neighbor_edges(&'b mut self, focus: &Focus) -> HashSet<NodeIndex> {
        // Removing other branching edges along the path.
        let mut remove = vec![];
        let edges = self
            .node(focus.from)
            .unwrap()
            .edges
            .iter()
            .filter(|e| e.from_position == focus.from_position);
        let remove_edges = edges.filter(|e| e.copy_number == Some(0));
        remove.extend(remove_edges.map(|e| e.norm_key()));
        for &(node, _) in focus.path.iter() {
            let edges = self
                .node(node)
                .unwrap()
                .edges
                .iter()
                .filter(|edge| edge.copy_number == Some(0))
                .map(|e| e.norm_key());
            remove.extend(edges);
        }
        for &(f, t) in remove.iter() {
            self.remove_edge_between(f, t);
        }
        remove.iter().flat_map(|(f, t)| [f.0, t.0]).collect()
    }
    fn clean_up(&'b mut self, focus: &Focus, mut affected: HashSet<NodeIndex>) {
        affected.extend(focus.path.iter().map(|n| n.0));
        for node in affected {
            if !self.is_deleted(node) {
                let edge_num = self.node(node).unwrap().edges.len();
                if edge_num == 0 {
                    self.delete(node);
                }
            }
        }
    }
    fn remove_along(&'b mut self, focus: &Focus) {
        self.remove_used_edges(focus);
        let affected = self.remove_neighbor_edges(focus);
        self.clean_up(focus, affected);
    }

    pub fn resolve_repeats(
        &'b mut self,
        reads: &[&EncodedRead],
        config: &AssembleConfig,
        thr: f64,
    ) {
        debug!("FOCI\tRESOLVE\t{:.3}\t{}", thr, config.min_span_reads);
        warn!("Please fix lk computation!");
        let mut count = 1;
        while count != 0 {
            let mut foci = self.get_foci(reads, config);
            let prev = foci.len();
            foci.retain(|val| thr < val.llr());
            foci.retain(|val| val.llr() != std::f64::INFINITY);
            if foci.is_empty() {
                break;
            }
            debug!("FOCI\tNUM\t{}\t{}", prev, foci.len());
            foci.sort_by(|x, y| match y.llr().partial_cmp(&x.llr()).unwrap() {
                std::cmp::Ordering::Equal => (y.dist, y.from).cmp(&(x.dist, x.from)),
                x => x,
            });
            count = self.survey_foci(&foci);
            debug!("FOCI\tTryAndSuccess\t{}\t{}", foci.len(), count);
        }
    }

    /// Return a hash map containing all foci, thresholded by `config` parameter.
    /// For each (node, position), keep the strongest focus.
    fn get_foci(&self, reads: &[&EncodedRead], config: &AssembleConfig) -> Vec<Focus> {
        let mut foci = vec![];
        for (index, node) in self
            .nodes()
            .filter(|(_, n)| matches!(n.copy_number, Some(1)))
        {
            // Find
            // --Node(Copy=1)--|
            //                 |--Node(Copy>1)--
            // --Node----------|
            //  (We are here now)
            for pos in [Position::Head, Position::Tail] {
                if self.count_edges(index, pos) != 1 {
                    continue;
                }
                let edge = node.edges.iter().find(|e| e.from_position == pos).unwrap();
                let siblings = self.count_edges(edge.to, edge.to_position);
                assert!(1 <= siblings);
                if siblings == 1 {
                    continue;
                }
                // If this edge does not flow into multi-copy contig, continue.
                match self.node(edge.to).unwrap().copy_number {
                    None | Some(1) | Some(0) => continue,
                    _ => {}
                }
                if let Some(focus) = self.examine_focus(index, pos, reads, config) {
                    foci.push(focus);
                }
            }
        }
        foci
    }
    fn split_node_info(
        &self,
        nodes: &[NodeWithTraverseInfo],
    ) -> (Vec<usize>, Vec<&DitchNode>, Vec<(NodeIndex, Position)>) {
        let (mut occs, mut raw_nodes, mut node_indices) = (vec![], vec![], vec![]);
        for (count, _, _, index, pos) in nodes.iter() {
            let raw_node = self.node(*index).unwrap();
            if 0 < raw_node.occ {
                occs.push(*count);
                raw_nodes.push(raw_node);
                node_indices.push((*index, *pos));
            }
        }
        (occs, raw_nodes, node_indices)
    }
    fn max_lk_node(&self, nodes: &[NodeWithTraverseInfo]) -> Option<(f64, (NodeIndex, Position))> {
        let (occs, raw_nodes, node_indices) = self.split_node_info(nodes);
        if occs.len() < 2 {
            return None;
        }
        let null_distr = normalize_coverage_array(&raw_nodes);
        let null_likelihood = lk_of_counts(&occs, &null_distr);
        assert!(!null_likelihood.is_nan(), "{:?}\t{:?}", null_distr, occs);
        assert!(null_likelihood.is_finite(), "{:?}\t{:?}", null_distr, occs);
        let (correct_lk, error_lk) = lk_pairs(occs.len());
        assert_eq!(occs.len(), node_indices.len());
        raw_nodes
            .iter()
            .zip(node_indices.iter())
            .enumerate()
            .filter(|(_, (n, _))| n.copy_number == Some(1))
            .map(|(k, (_, &idx))| {
                let occs_with_idx = occs.iter().map(|&x| x as f64).enumerate();
                let lk: f64 = occs_with_idx
                    .map(|(i, occ)| occ * (if i == k { correct_lk } else { error_lk }))
                    .sum();
                assert!(!lk.is_nan());
                (lk - null_likelihood, idx)
            })
            .max_by(|x, y| (x.0).partial_cmp(&(y.0)).unwrap())
    }

    fn count_dist_nodes(
        &self,
        reads: &[&EncodedRead],
        node_index: NodeIndex,
        pos: Position,
    ) -> Vec<HashMap<Node, usize>> {
        let max_len: usize = reads.iter().map(|r| r.nodes.len()).max().unwrap();
        let mut node_counts_at = vec![HashMap::new(); max_len + 1];
        let node = self.node(node_index).unwrap().node;
        for read in reads.iter() {
            let start = read
                .nodes
                .iter()
                .position(|n| (n.unit, n.cluster) == node)
                .unwrap();
            match (read.nodes[start].is_forward, pos) {
                (true, Position::Tail) | (false, Position::Head) => {
                    // ---> (Here to start) -- <---> -- <---> .... Or
                    // <--- (Here to start) -- <---> -- <---> ....
                    for (d, node) in read.nodes.iter().skip(start).enumerate() {
                        *node_counts_at[d]
                            .entry((node.unit, node.cluster))
                            .or_default() += 1;
                    }
                }
                (true, Position::Head) | (false, Position::Tail) => {
                    let read = read.nodes.iter().take(start + 1).rev();
                    for (d, node) in read.enumerate() {
                        *node_counts_at[d]
                            .entry((node.unit, node.cluster))
                            .or_default() += 1;
                    }
                }
            }
        }
        node_counts_at
    }
    fn next_nodes(&self, nodes: &[NodeWithTraverseInfo]) -> Vec<(NodeIndex, Position)> {
        let mut found_nodes: Vec<_> = vec![];
        for &(_, _, _, index, pos) in nodes {
            let edges = self.edges_from(index, !pos).into_iter();
            let edges = edges.filter(|e| matches!(e.copy_number,Some(cp) if cp > 0));
            for edge in edges {
                found_nodes.push((edge.to, edge.to_position));
            }
        }
        found_nodes.sort_unstable();
        found_nodes.dedup();
        found_nodes
    }
    // Dist -> Vec<(count, max_so_far, parent, node_index, pos)>
    fn traverse(
        &self,
        reads: &[&EncodedRead],
        node_index: NodeIndex,
        pos: Position,
        min_span: usize,
    ) -> Vec<Vec<NodeWithTraverseInfo>> {
        let node_counts_at = self.count_dist_nodes(reads, node_index, pos);
        let mut dist_and_maxweight = vec![vec![(0, 0, 0, node_index, !pos)]];
        for dist in 0.. {
            let prev_nodes = &dist_and_maxweight[dist];
            let found_nodes = self.next_nodes(prev_nodes);
            let map_to_idx = to_btree_map(&found_nodes);
            let mut maxweight: Vec<_> = found_nodes
                .iter()
                .map(|&(idx, pos)| {
                    let node = self.node(idx).unwrap().node;
                    let count = *node_counts_at[dist + 1].get(&node).unwrap_or(&0);
                    (count, count, 0, idx, pos)
                })
                .collect();
            for (i, &(_, max, _, index, pos)) in prev_nodes.iter().enumerate() {
                let edges = self.edges_from(index, !pos).into_iter();
                let edges = edges.filter(|e| matches!(e.copy_number,Some(cp) if cp > 0));
                let to_locations = edges.filter_map(|e| map_to_idx.get(&(e.to, e.to_position)));
                for &to_location in to_locations {
                    let old_max = maxweight.get_mut(to_location).unwrap();
                    if old_max.1 < max + old_max.0 {
                        *old_max = (old_max.0, old_max.0 + max, i, old_max.3, old_max.4);
                    }
                }
            }
            if maxweight.iter().map(|x| x.0).sum::<usize>() < min_span {
                break;
            }
            dist_and_maxweight.push(maxweight);
        }
        dist_and_maxweight
    }
    fn trackback(
        indices_and_parents: &[Vec<NodeWithTraverseInfo>],
        mut dist: usize,
        target: (NodeIndex, Position),
    ) -> Vec<(NodeIndex, Position)> {
        let mut backpath = vec![];
        let (mut target, _) = indices_and_parents[dist]
            .iter()
            .enumerate()
            .find(|&(_, &(_, _, _, n, p))| (n, p) == target)
            .unwrap();
        while 0 < dist {
            let (_, _, _, n, p) = indices_and_parents[dist][target];
            backpath.push((n, p));
            target = indices_and_parents[dist][target].2;
            dist -= 1;
        }
        backpath.reverse();
        backpath
    }
    fn examine_focus(
        &self,
        node_index: NodeIndex,
        pos: Position,
        reads: &[&EncodedRead],
        config: &AssembleConfig,
    ) -> Option<Focus> {
        let node = self.node(node_index).unwrap();
        let min_span = config.min_span_reads;
        let reads: Vec<_> = reads
            .iter()
            .filter(|r| r.contains(node.node))
            .copied()
            .collect();
        let indices_and_parents = self.traverse(&reads, node_index, pos, min_span);
        indices_and_parents
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(d, nodes_with_pars)| {
                self.max_lk_node(nodes_with_pars)
                    .map(|(lk, target)| (lk, d, target))
            })
            .max_by(|x, y| x.0.partial_cmp(&y.0).unwrap())
            .map(|(lk, d, (to_idx, to_pos))| {
                let backpath = Self::trackback(&indices_and_parents, d, (to_idx, to_pos));
                let nodes_with_pars = &indices_and_parents[d];
                let counts: Vec<_> = nodes_with_pars.iter().map(|x| x.0).collect();
                let to_node = self.node(to_idx).unwrap().node;
                let from_tuple = (node_index, node.node, pos);
                let to_tuple = (to_idx, to_node, to_pos);
                Focus::with_backpath(from_tuple, to_tuple, d, lk, counts, backpath)
            })
    }
    fn survey_focus(&'b mut self, focus: &Focus, affected: &mut HashSet<NodeIndex>) -> Option<()> {
        if !self.is_path_branching(focus) {
            return None;
        }
        for (i, &(index, pos)) in focus.path.iter().enumerate() {
            let node = self.node(index).unwrap().node;
            debug!("FOCUS\tTrace\t{}\t{:?}\t{}", i, node, pos);
        }
        let new_nodes = self.duplicate_along(focus);
        affected.extend(new_nodes);
        affected.extend(focus.path.iter().map(|x| x.0));
        affected.insert(focus.from);
        self.remove_along(focus);
        Some(())
    }
}
fn lk_pairs(len: usize) -> (f64, f64) {
    let choice_num = len as f64;
    let correct_lk = ((1f64 - ERROR_PROB).powi(2) + ERROR_PROB / choice_num).ln();
    let error_lk = {
        let correct_to_error = (1.0 - ERROR_PROB) * ERROR_PROB * (choice_num - 1.0) / choice_num;
        let error_to_error =
            ERROR_PROB / choice_num * ERROR_PROB * (choice_num - 1f64) / choice_num;
        (correct_to_error + error_to_error).ln()
    };
    assert!(!correct_lk.is_nan(), "{}", len);
    assert!(!error_lk.is_nan(), "{}", len);
    (correct_lk, error_lk)
}

fn to_btree_map(nodes: &[(NodeIndex, Position)]) -> BTreeMap<(NodeIndex, Position), usize> {
    nodes.iter().enumerate().map(|(i, &n)| (n, i)).collect()
}

fn normalize_coverage_array(nodes: &[&DitchNode]) -> Vec<f64> {
    let mut probs: Vec<_> = nodes.iter().map(|n| n.occ as f64).collect();
    let sum: f64 = probs.iter().sum();
    probs.iter_mut().for_each(|x| *x = (*x / sum).ln());
    assert!(probs.iter().all(|x| !x.is_nan()), "{:?}", probs);
    probs
}

fn lk_of_counts(occs: &[usize], distr: &[f64]) -> f64 {
    occs.iter()
        .zip(distr)
        .filter(|x| 0 < *x.0)
        .map(|(&occ, ln)| occ as f64 * ln)
        .sum()
}
