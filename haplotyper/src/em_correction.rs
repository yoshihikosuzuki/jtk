use definitions::*;
use rayon::prelude::*;
// const SEED: u64 = 1221;
use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256StarStar;
use std::collections::HashMap;
use std::collections::HashSet;
#[derive(Debug, Clone)]
pub struct Config {
    repeat_num: usize,
    seed: u64,
    cluster_num: usize,
    coverage_thr: usize,
    focal: u64,
}

impl Config {
    pub fn new(
        repeat_num: usize,
        seed: u64,
        cluster_num: usize,
        focal: u64,
        coverage: usize,
    ) -> Self {
        Self {
            repeat_num,
            seed,
            cluster_num,
            focal,
            coverage_thr: coverage,
        }
    }
}

pub trait ClusteringCorrection {
    // fn correct_clustering(self, repeat_num: usize, coverage_thr: usize) -> Self;
    fn correct_clustering_em(self, repeat_num: usize, coverage_thr: usize, len_thr: usize) -> Self;
}

impl ClusteringCorrection for DataSet {
    // fn correct_clustering(mut self, repeat_num: usize, coverage_thr: usize) -> Self {
    //     let id_to_name: HashMap<_, _> = self.raw_reads.iter().map(|r| (r.id, &r.name)).collect();
    //     let id_to_desc: HashMap<_, _> = self.raw_reads.iter().map(|r| (r.id, &r.desc)).collect();
    //     let result: Vec<_> = self
    //         .selected_chunks
    //         .par_iter()
    //         .flat_map(|ref_unit| {
    //             let unit_id = ref_unit.id;
    //             let (read_indices, reads): (Vec<usize>, Vec<_>) = self
    //                 .encoded_reads
    //                 .iter()
    //                 .enumerate()
    //                 .filter(|(_, r)| r.nodes.iter().any(|n| n.unit == unit_id))
    //                 .unzip();
    //             let k = ref_unit.cluster_num;
    //             let config = Config::new(repeat_num, SEED, k, unit_id, coverage_thr);
    //             if reads.is_empty() {
    //                 debug!("Unit {} does not appear in any read.", unit_id);
    //                 return vec![];
    //             }
    //             let new_clustering = clustering(&self.selected_chunks, &reads, &config);
    //             if log_enabled!(log::Level::Debug) {
    //                 for cl in 0..k {
    //                     for (read, &cluster) in reads.iter().zip(new_clustering.iter()) {
    //                         let id = read.id;
    //                         if cluster == cl {
    //                             let name = id_to_name[&id];
    //                             let desc = match id_to_desc.get(&id) {
    //                                 Some(res) => res.as_str(),
    //                                 None => "",
    //                             };
    //                             debug!("IMP\t{}\t{}\t{}\t{}\t{}", unit_id, cl, id, name, desc);
    //                         }
    //                     }
    //                 }
    //             }
    //             let mut result = vec![];
    //             for ((read, read_idx), cluster) in
    //                 reads.into_iter().zip(read_indices).zip(new_clustering)
    //             {
    //                 for (idx, node) in read.nodes.iter().enumerate() {
    //                     if node.unit == unit_id {
    //                         result.push((read_idx, read.id, idx, node.unit, cluster));
    //                     }
    //                 }
    //             }
    //             result
    //         })
    //         .collect();
    //     for (read_idx, read_id, position, unit_id, cluster) in result {
    //         assert_eq!(self.encoded_reads[read_idx].id, read_id);
    //         assert_eq!(self.encoded_reads[read_idx].nodes[position].unit, unit_id);
    //         self.encoded_reads[read_idx].nodes[position].cluster = cluster as u64;
    //     }
    //     self
    // }
    fn correct_clustering_em(
        mut self,
        repeat_num: usize,
        coverage_thr: usize,
        len_thr: usize,
    ) -> Self {
        let (result, cluster_size_and_lk): (Vec<_>, Vec<_>) = self
            .selected_chunks
            .par_iter()
            .map(|ref_unit| {
                let unit_id = ref_unit.id;
                let reads: Vec<_> = self
                    .encoded_reads
                    .iter()
                    .filter(|r| r.nodes.iter().any(|n| n.unit == unit_id))
                    .collect();
                let k = ref_unit.cluster_num;
                if reads.is_empty() {
                    debug!("Unit {} does not appear in any read.", unit_id);
                    return (vec![], (0f64, 0));
                }
                let (new_clustering, lk, cluster_num) = (0..repeat_num as u64)
                    .map(|s| {
                        let config = Config::new(repeat_num, unit_id * s, k, unit_id, coverage_thr);
                        em_clustering(&reads, &config)
                    })
                    .max_by(|x, y| (x.1).partial_cmp(&(y.1)).unwrap())
                    .unwrap();
                (new_clustering, (lk, cluster_num))
            })
            .unzip();
        let cluster_size: Vec<_> = cluster_size_and_lk.iter().map(|x| x.1).collect();
        let lk: f64 = cluster_size_and_lk.iter().map(|x| x.0).sum();
        debug!("Total LK:{:.2}", lk);
        self.selected_chunks
            .iter_mut()
            .zip(cluster_size)
            .for_each(|(unit, c)| unit.cluster_num = c);
        let result: HashMap<u64, Vec<(usize, u64)>> =
            result.iter().fold(HashMap::new(), |mut acc, results| {
                for &(id, pos, cluster) in results {
                    acc.entry(id).or_default().push((pos, cluster));
                }
                acc
            });
        for read in self
            .encoded_reads
            .iter_mut()
            .filter(|r| len_thr <= r.nodes.len())
        {
            if let Some(corrected) = result.get(&read.id) {
                for &(pos, cluster) in corrected {
                    read.nodes[pos].cluster = cluster;
                }
            }
        }
        self
    }
}

fn logsumexp(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.;
    } else if xs.len() == 1 {
        xs[0]
    } else {
        let max = xs.iter().max_by(|x, y| x.partial_cmp(&y).unwrap()).unwrap();
        let sum = xs.iter().map(|x| (x - max).exp()).sum::<f64>().ln();
        assert!(sum >= 0., "{:?}->{}", xs, sum);
        max + sum
    }
}

/// Return the id of the read, the position at that read, and the clusters predicted.
pub fn em_clustering(
    reads: &[&EncodedRead],
    config: &Config,
) -> (Vec<(u64, usize, u64)>, f64, usize) {
    trace!("==================");
    let mut unit_counts: HashMap<_, usize> = HashMap::new();
    for read in reads.iter() {
        for node in read.nodes.iter() {
            *unit_counts.entry(node.unit).or_default() += 1;
        }
    }
    let use_units: HashSet<_> = unit_counts
        .iter()
        .filter(|&(_, &c)| c > config.coverage_thr)
        .map(|(&x, _)| x)
        .collect();
    let contexts: Vec<_> = {
        let mut buffer = vec![];
        for read in reads.iter() {
            for index in 0..read.nodes.len() {
                if read.nodes[index].unit == config.focal {
                    buffer.push(Context::new(read, index, &use_units));
                }
            }
        }
        buffer
    };
    let mut rng: Xoshiro256StarStar = SeedableRng::seed_from_u64(config.seed);
    // let min_cluster = config.cluster_num.max(3) - 2;
    // let max_cluster = config.cluster_num + 2;
    // (min_cluster..max_cluster)
    //     .flat_map(|cluster_num| std::iter::repeat(cluster_num).take(10))
    std::iter::repeat(config.cluster_num)
        .take(10)
        .map(|cluster_num| {
            let (asn, lk) = em_clustering_inner(&contexts, cluster_num, &mut rng);
            // Num of parameters.
            let offset = (cluster_num * (use_units.len() - 1)) as f64;
            let lk = lk - offset;
            (asn, lk, cluster_num)
        })
        .max_by(|x, y| (x.1).partial_cmp(&(y.1)).unwrap())
        .map(|(asn, lk, cluster_num)| (asn, lk, cluster_num))
        .unwrap()
}

pub fn initialize_weights<R: Rng>(contexts: &[Context], k: usize, rng: &mut R) -> Vec<Vec<f64>> {
    fn norm(c1: &Context, c2: &Context) -> f64 {
        let center_match = (c1.cluster != c2.cluster) as u32;
        let (f, flen) = c1
            .forward
            .iter()
            .zip(c2.forward.iter())
            .fold((0, 0), |(acc, len), (c1, c2)| {
                (acc + (c1 != c2) as u32, len + 1)
            });
        let (b, blen) = c1
            .backward
            .iter()
            .zip(c2.backward.iter())
            .fold((0, 0), |(acc, len), (c1, c2)| {
                (acc + (c1 != c2) as u32, len + 1)
            });
        (center_match + b + f) as f64 / (1 + flen + blen) as f64
    }
    let mut centers: Vec<&Context> = vec![];
    let indices: Vec<_> = (0..contexts.len()).collect();
    // Choosing centers.
    use rand::seq::SliceRandom;
    while centers.len() < k as usize {
        // calculate distance to the most nearest centers.
        let mut dists: Vec<_> = contexts
            .iter()
            .map(|ctx| {
                centers
                    .iter()
                    .map(|c| norm(ctx, c))
                    .min_by(|x, y| x.partial_cmp(y).unwrap())
                    .unwrap_or(1f64)
                    .powi(2)
            })
            .collect();
        let total: f64 = dists.iter().sum();
        dists.iter_mut().for_each(|x| *x /= total);
        let idx = *indices.choose_weighted(rng, |&idx| dists[idx]).unwrap();
        centers.push(&contexts[idx]);
    }
    contexts
        .iter()
        .map(|ctx| {
            let idx = centers
                .iter()
                .enumerate()
                .map(|(i, c)| (i, norm(c, ctx)))
                .min_by(|x, y| x.1.partial_cmp(&(y.1)).unwrap())
                .unwrap()
                .0;
            let mut weights = vec![100f64; k];
            weights[idx] += 100f64;
            let sum: f64 = weights.iter().sum();
            weights.iter_mut().for_each(|x| *x /= sum);
            weights
        })
        .collect()
}
pub fn em_clustering_inner<R: Rng>(
    contexts: &[Context],
    k: usize,
    rng: &mut R,
) -> (Vec<(u64, usize, u64)>, f64) {
    let mut weights = initialize_weights(contexts, k, rng);
    // let mut weights: Vec<_> = (0..contexts.len())
    //     .map(|_| {
    //         let mut weight = vec![0f64; k];
    //         const NUM: usize = 100;
    //         for _ in 0..NUM {
    //             weight[rng.gen_range(0..k)] += 1f64;
    //         }
    //         weight.iter_mut().for_each(|x| *x /= NUM as f64);
    //         weight
    //     })
    //     .collect();
    let mut model = EMModel::new(&contexts, &weights, k);
    let mut lk: f64 = contexts
        .iter()
        .map(|ctx| model.get_weight(ctx, &mut vec![]).1)
        .sum();
    trace!("LK:{}", lk);
    loop {
        let (prev_model, prev_weights) = (model.clone(), weights.clone());
        model.update(&mut weights, &contexts);
        let next_lk = contexts
            .iter()
            .map(|ctx| model.get_weight(ctx, &mut vec![]).1)
            .sum();
        trace!("LK:{}", next_lk);
        if (next_lk - lk) < 0.001 {
            model = prev_model;
            weights = prev_weights;
            break;
        } else {
            lk = next_lk;
        }
    }
    let predictions: Vec<_> = contexts
        .iter()
        .zip(weights.iter())
        .map(|(ctx, ws)| {
            let cluster: usize = ws
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .map(|x| x.0)
                .unwrap();
            (ctx.id, ctx.index, cluster as u64)
        })
        .collect();
    trace!("MODEL:{}", model);
    for (weight, context) in weights.iter().zip(contexts.iter()) {
        let weight: Vec<_> = weight.iter().map(|x| format!("{:.3}", x)).collect();
        trace!("{}\t{}", context.id, weight.join("\t"));
    }
    (predictions, lk)
}

// TODO: Add direction.
#[derive(Debug, Clone)]
pub struct Context {
    // The ID of the original encoded read.
    id: u64,
    // The original index of this context.
    index: usize,
    unit: u64,
    cluster: u64,
    forward: Vec<(u64, u64)>,
    backward: Vec<(u64, u64)>,
}

impl Context {
    pub fn with_attrs(
        id: u64,
        index: usize,
        unit: u64,
        cluster: u64,
        forward: Vec<(u64, u64)>,
        backward: Vec<(u64, u64)>,
    ) -> Self {
        Self {
            id,
            index,
            unit,
            cluster,
            forward,
            backward,
        }
    }
    fn new(read: &EncodedRead, index: usize, use_unit: &HashSet<u64>) -> Self {
        let (unit, cluster) = (read.nodes[index].unit, read.nodes[index].cluster);
        let nodes = read.nodes.iter();
        let forward: Vec<_> = nodes
            .clone()
            .skip(index + 1)
            .map(|n| (n.unit, n.cluster))
            .filter(|n| use_unit.contains(&n.0))
            .collect();
        let backward: Vec<_> = nodes
            .clone()
            .take(index)
            .rev()
            .map(|n| (n.unit, n.cluster))
            .filter(|n| use_unit.contains(&n.0))
            .collect();
        if read.nodes[index].is_forward {
            Self {
                id: read.id,
                index,
                unit,
                cluster,
                forward,
                backward,
            }
        } else {
            Self {
                id: read.id,
                index,
                unit,
                cluster,
                forward: backward,
                backward: forward,
            }
        }
    }
}

#[derive(Debug, Clone)]
struct EMModel {
    center_unit: u64,
    cluster_num: HashMap<u64, u64>,
    fraction: Vec<f64>,
    models: Vec<RawModel>,
}

impl std::fmt::Display for EMModel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "{}", self.cluster_num[&self.center_unit])?;
        for (frac, m) in self.fraction.iter().zip(self.models.iter()) {
            writeln!(f, "{:.3}\n{}", frac, m)?;
        }
        Ok(())
    }
}

impl EMModel {
    // Create new model.
    fn new(contexts: &[Context], weights: &[Vec<f64>], cluster_num: usize) -> Self {
        let mut fraction = vec![0f64; cluster_num];
        for weight in weights.iter() {
            fraction.iter_mut().zip(weight).for_each(|(x, y)| *x += y);
        }
        let sum: f64 = fraction.iter().sum();
        fraction.iter_mut().for_each(|x| *x /= sum);
        let mut models = vec![RawModel::new(); cluster_num];
        for (weight, context) in weights.iter().zip(contexts) {
            models.iter_mut().zip(weight).for_each(|(model, &w)| {
                if SMALL < w {
                    model.add(w, context);
                }
            });
        }
        models.iter_mut().for_each(|model| model.normalize());
        let center_unit = contexts[0].unit;
        let mut cluster_num: HashMap<_, u64> =
            std::iter::once((contexts[0].unit, cluster_num as u64)).collect();
        for context in contexts {
            for &(unit, cluster) in context.forward.iter() {
                let x = cluster_num.entry(unit).or_default();
                *x = (*x).max(cluster + 1);
            }
            for &(unit, cluster) in context.backward.iter() {
                let x = cluster_num.entry(unit).or_default();
                *x = (*x).max(cluster + 1);
            }
        }
        Self {
            center_unit,
            cluster_num,
            fraction,
            models,
        }
    }
    fn clear(&mut self) {
        self.fraction.iter_mut().for_each(|x| *x = 0f64);
        self.models.iter_mut().for_each(|m| m.clear());
    }
    // Return the **previous** likelihood!!!!!!!!!!
    fn update(&mut self, weights: &mut [Vec<f64>], contexts: &[Context]) -> f64 {
        let mut total_lk = 0f64;
        // Calculate the next weights, mapping position, and likelihood.
        let mapping_positions: Vec<_> = weights
            .iter_mut()
            .zip(contexts.iter())
            .map(|(weight, context)| {
                let (mapping_positions, lk) = self.get_weight(context, weight);
                total_lk += lk;
                mapping_positions
            })
            .collect();
        // Clear the previous model.
        self.clear();
        // Update fraction.
        for weight in weights.iter() {
            for (i, w) in weight.iter().enumerate() {
                self.fraction[i] += w;
            }
        }
        let sum: f64 = self.fraction.iter().sum();
        self.fraction.iter_mut().for_each(|x| *x /= sum);
        // Update paramters.
        let summaries = weights.iter().zip(contexts).zip(mapping_positions);
        for ((weight, context), mapping_positions) in summaries {
            let each_model = weight
                .iter()
                .zip(self.models.iter_mut())
                .zip(mapping_positions);
            for ((&w, model), (forward_map, backward_map)) in each_model {
                if SMALL < w {
                    *model.center.entry(context.cluster).or_default() += w;
                    for (&elm, pos) in context.forward.iter().zip(forward_map) {
                        *model.forward[pos].entry(elm).or_default() += w;
                    }
                    for (&elm, pos) in context.backward.iter().zip(backward_map) {
                        *model.backward[pos].entry(elm).or_default() += w;
                    }
                }
            }
        }
        self.models.iter_mut().for_each(|model| model.normalize());
        total_lk
    }
    // Mapping position is the location of the j-th element to the i-th model.[i][j]
    fn get_weight(
        &self,
        context: &Context,
        weight: &mut [f64],
    ) -> (Vec<(Vec<usize>, Vec<usize>)>, f64) {
        let (lks, mapping_positions): (Vec<_>, Vec<_>) = self
            .models
            .iter()
            .zip(self.fraction.iter())
            .map(|(m, f)| {
                // TODO: change here.
                // let center_lk = m.center[&context.cluster].max(SMALL).ln();
                let center_lk = (MISM_PROB * (self.cluster_num[&context.unit] as f64).recip()
                    + (1f64 - MISM_PROB) * m.center.get(&context.cluster).unwrap_or(&SMALL))
                .ln();
                let (forward_mapping, lkf) = m.forward_mapping(context);
                let (backward_mapping, lkb) = m.backward_mapping(context);
                let lk = f.max(SMALL).ln() + center_lk + lkf + lkb;
                (lk, (forward_mapping, backward_mapping))
            })
            .unzip();
        let total_lk = logsumexp(&lks);
        weight
            .iter_mut()
            .zip(lks)
            .for_each(|(w, lk)| *w = (lk - total_lk).exp());
        (mapping_positions, total_lk)
    }
}

const DEL_PROB: f64 = 0.02;
const MISM_PROB: f64 = 0.02;
// const MATCH_PROB: f64 = 1f64 - DEL_PROB ;
const MATCH_PROB: f64 = 1f64 - DEL_PROB - MISM_PROB;
const SMALL: f64 = 0.00001;

#[derive(Debug, Clone)]
struct RawModel {
    center: HashMap<u64, f64>,
    forward: Vec<HashMap<(u64, u64), f64>>,
    backward: Vec<HashMap<(u64, u64), f64>>,
}

impl std::fmt::Display for RawModel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Center:{:?}", self.center)?;
        for (i, fr) in self.forward.iter().enumerate() {
            let dump: Vec<_> = fr
                .iter()
                .map(|((x, y), val)| format!("({}-{}):{:.3}", x, y, val))
                .collect();
            writeln!(f, "F{}:{}", i, dump.join("\t"))?;
        }
        for (i, b) in self.backward.iter().enumerate() {
            let dump: Vec<_> = b
                .iter()
                .map(|((x, y), val)| format!("({}-{}):{:.3}", x, y, val))
                .collect();
            writeln!(f, "B{}:{}", i, dump.join("\t"))?;
        }
        Ok(())
    }
}

impl RawModel {
    fn new() -> Self {
        Self {
            center: HashMap::new(),
            forward: vec![],
            backward: vec![],
        }
    }
    // Add `context` into self. The position is the same as context.
    fn add(&mut self, w: f64, context: &Context) {
        // Enlarge model if needed.
        let forward_len = context.forward.len().saturating_sub(self.forward.len());
        self.forward
            .extend(std::iter::repeat(HashMap::new()).take(forward_len));
        let backward_len = context.backward.len().saturating_sub(self.backward.len());
        self.backward
            .extend(std::iter::repeat(HashMap::new()).take(backward_len));
        // Add center.
        *self.center.entry(context.cluster).or_default() += w;
        // Add context.
        self.forward
            .iter_mut()
            .zip(context.forward.iter())
            .for_each(|(slot, &elm)| *slot.entry(elm).or_default() += w);
        self.backward
            .iter_mut()
            .zip(context.backward.iter())
            .for_each(|(slot, &elm)| *slot.entry(elm).or_default() += w);
    }
    // fn lk(&self, context: &Context) -> f64 {
    //     let center_lk = self.center[&context.cluster].max(SMALL).ln();
    //     self.forward_mapping(context).1 + center_lk + self.backward_mapping(context).1
    // }
    // Normalize this model to stat model.
    fn normalize(&mut self) {
        let sum: f64 = self.center.values().sum();
        self.center.values_mut().for_each(|x| *x /= sum);
        fn normalize(slots: &mut HashMap<(u64, u64), f64>) {
            let sum: f64 = slots.values().sum();
            slots.values_mut().for_each(|x| *x /= sum);
            slots.retain(|_, prob| SMALL < *prob);
        }
        self.forward.iter_mut().for_each(normalize);
        self.backward.iter_mut().for_each(normalize);
    }
    // Clear all the weights.
    fn clear(&mut self) {
        self.center.values_mut().for_each(|x| *x = 0f64);
        fn clear(slots: &mut HashMap<(u64, u64), f64>) {
            slots.values_mut().for_each(|x| *x = 0f64);
        }
        self.forward.iter_mut().for_each(clear);
        self.backward.iter_mut().for_each(clear);
    }
    fn align_profile(
        elements: &[(u64, u64)],
        profiles: &[HashMap<(u64, u64), f64>],
    ) -> (Vec<usize>, f64) {
        // assert!(
        //     elements.len() <= profiles.len(),
        //     "{},{}",
        //     elements.len(),
        //     profiles.len()
        // );
        let minimum = SMALL.ln() * (elements.len() * profiles.len()) as f64;
        let mut dp = vec![vec![minimum; profiles.len() + 1]; elements.len() + 1];
        // True for match, false for deletion.
        let mut is_matched = vec![vec![false; profiles.len() + 1]; elements.len() + 1];
        for j in 0..profiles.len() {
            dp[0][j] = j as f64 * DEL_PROB.ln();
        }
        for (i, elm) in elements.iter().enumerate().map(|(i, p)| (i + 1, p)) {
            for (j, slot) in profiles.iter().enumerate().map(|(j, p)| (j + 1, p)) {
                // If there is no unit, then we use uniformal distribution.
                let match_prob = MATCH_PROB * slot.get(elm).copied().unwrap_or(0.5);
                let mismatch_prob = MISM_PROB * 0.5;
                // Modified.
                let match_score = dp[i - 1][j - 1] + (match_prob + mismatch_prob).max(SMALL).ln();
                let del_score = dp[i][j - 1] + DEL_PROB.ln();
                if del_score < match_score {
                    dp[i][j] = match_score;
                    is_matched[i][j] = true;
                } else {
                    dp[i][j] = del_score;
                    is_matched[i][j] = false;
                }
            }
        }
        // Start from the corner.
        let mut mapping_position = vec![0; elements.len()];
        let mut i = elements.len();
        let (mut j, lk) = dp[i]
            .iter()
            .enumerate()
            .max_by(|x, y| (x.1).partial_cmp(&(y.1)).unwrap())
            .unwrap();
        while 0 < i && 0 < j {
            if is_matched[i][j] {
                assert!(0 < j && 0 < i, "{},{}", i, j);
                i -= 1;
                j -= 1;
                mapping_position[i] = j;
            } else {
                assert!(0 < j, "{},{}", i, j);
                j -= 1;
            }
        }
        while 0 < i {
            i -= 1;
            mapping_position[i] = j;
        }
        while 0 < j {
            j -= 1;
        }
        assert_eq!(i, 0);
        assert_eq!(j, 0);
        (mapping_position, *lk)
    }
    // Return the mapping position of each unit in the forward direction of context.
    // In other words, the i-th element would be the index of the self.forward such that
    // the location would give the maximum likelihood.
    fn forward_mapping(&self, context: &Context) -> (Vec<usize>, f64) {
        Self::align_profile(&context.forward, &self.forward)
    }
    // Return the mapping position of each unit in the backward direction of context.
    // In other words, the i-th element would be the index of the self.backward such that
    // the location would give the maximum likelihood.
    fn backward_mapping(&self, context: &Context) -> (Vec<usize>, f64) {
        Self::align_profile(&context.backward, &self.backward)
    }
}

#[cfg(test)]
mod test {}
