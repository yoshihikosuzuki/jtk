use definitions::*;
use rayon::prelude::*;
const SEED: u64 = 1221;
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
    fn correct_clustering(self, repeat_num: usize, coverage_thr: usize) -> Self;
}

impl ClusteringCorrection for DataSet {
    fn correct_clustering(mut self, repeat_num: usize, coverage_thr: usize) -> Self {
        let id_to_name: HashMap<_, _> = self.raw_reads.iter().map(|r| (r.id, &r.name)).collect();
        let id_to_desc: HashMap<_, _> = self.raw_reads.iter().map(|r| (r.id, &r.desc)).collect();
        let result: Vec<_> = self
            .selected_chunks
            .par_iter()
            .flat_map(|ref_unit| {
                let unit_id = ref_unit.id;
                let (read_indices, reads): (Vec<usize>, Vec<_>) = self
                    .encoded_reads
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.nodes.iter().any(|n| n.unit == unit_id))
                    .unzip();
                let k = ref_unit.cluster_num;
                let config = Config::new(repeat_num, SEED, k, unit_id, coverage_thr);
                if reads.is_empty() {
                    debug!("Unit {} does not appear in any read.", unit_id);
                    return vec![];
                }
                let new_clustering = clustering(&self.selected_chunks, &reads, &config);
                if log_enabled!(log::Level::Debug) {
                    for cl in 0..k {
                        for (read, &cluster) in reads.iter().zip(new_clustering.iter()) {
                            let id = read.id;
                            if cluster == cl {
                                let name = id_to_name[&id];
                                let desc = match id_to_desc.get(&id) {
                                    Some(res) => res.as_str(),
                                    None => "",
                                };
                                debug!("IMP\t{}\t{}\t{}\t{}\t{}", unit_id, cl, id, name, desc);
                            }
                        }
                    }
                }
                let mut result = vec![];
                for ((read, read_idx), cluster) in
                    reads.into_iter().zip(read_indices).zip(new_clustering)
                {
                    for (idx, node) in read.nodes.iter().enumerate() {
                        if node.unit == unit_id {
                            result.push((read_idx, read.id, idx, node.unit, cluster));
                        }
                    }
                }
                result
            })
            .collect();
        for (read_idx, read_id, position, unit_id, cluster) in result {
            assert_eq!(self.encoded_reads[read_idx].id, read_id);
            assert_eq!(self.encoded_reads[read_idx].nodes[position].unit, unit_id);
            self.encoded_reads[read_idx].nodes[position].cluster = cluster as u64;
        }
        self
    }
}

pub fn clustering(units: &[Unit], reads: &[&EncodedRead], config: &Config) -> Vec<usize> {
    let weights: Vec<_> = reads
        .iter()
        .map(|r| r.nodes.iter().find(|n| n.unit == config.focal).unwrap())
        .map(|node| {
            let mut weight = vec![0.; config.cluster_num];
            weight[node.cluster as usize] += 1.;
            weight
        })
        .collect();
    let mut rng: Xoshiro256StarStar = SeedableRng::seed_from_u64(config.focal * SEED);
    let (asn, lk) = em_clustering_old(units, reads, weights, config);
    let (asn, lk) = (0..config.repeat_num)
        .map(|_| {
            let weights: Vec<_> = reads
                .iter()
                .map(|_| {
                    let mut weight = vec![0.; config.cluster_num];
                    weight[rng.gen_range(0..config.cluster_num)] += 1.;
                    weight
                })
                .collect();
            em_clustering_old(units, reads, weights, config)
        })
        .fold((asn, lk), |(argmax, max_lk), (arg, lk)| {
            if lk < max_lk {
                (argmax, max_lk)
            } else {
                (arg, lk)
            }
        });
    trace!("OPTLK\t{}", lk);
    asn
}

// struct AdjInformation {
//     plus_direction: Vec<(usize, bool, usize)>,
//     minus_direction: Vec<(usize, bool, usize)>,
//     focal: (usize, bool, usize),
// }

// impl AdjInformation {
//     fn get(&self, i: isize) -> (usize, bool, usize) {
//         if i == 0 {
//             self.focal
//         } else if 0 < i {
//             self.plus_direction[i as usize]
//         } else {
//             self.minus_direction[(-i) as usize]
//         }
//     }
// }

// By accessing [i][j][bool], obtain what th i-th read has at the j-th index in bool strand.
// It consists of (unit, cluster)
// fn map_unit_and_clusters<T: std::borrow::Borrow<[(u64, bool, u64)]>>(
//     reads: &[T],
// ) -> Vec<Vec<(usize, bool, usize)>> {
//     let mut chunk_mapper: HashMap<u64, usize> = HashMap::new();
//     let mut cluster_mapper: Vec<HashMap<u64, usize>> = vec![];
//     for read in reads.iter() {
//         for &(chunk, cluster) in read.borrow().iter() {
//             let chunk_idx: usize = *chunk_mapper.entry(chunk).or_insert_with(|| {
//                 // First chunk.
//                 cluster_mapper.push(HashMap::new());
//                 cluster_mapper.len() - 1
//             });
//             // Never panic.
//             let cluster_num = cluster_mapper[chunk_idx].len();
//             if !cluster_mapper[chunk_idx].contains_key(&cluster) {
//                 cluster_mapper[chunk_idx].insert(cluster, cluster_num);
//             }
//         }
//     }
//     reads
//         .iter()
//         .map(|read| {
//             read.borrow()
//                 .iter()
//                 .map(|&(chunk, cluster)| {
//                     let chunk = chunk_mapper[&chunk];
//                     let cluster = cluster_mapper[chunk][&cluster];
//                     (chunk, cluster)
//                 })
//                 .collect()
//         })
//         .collect()
// }

/// Descrete EM algorithm
/// The model is the bugs of categorical distributions for
/// each cluster.
/// In other words, we have a categorical distribution on each chunk on each cluster, and
/// making the whole model as a mixture of them.
pub fn em_clustering<T: std::borrow::Borrow<[(u64, bool, u64)]>>(
    _reads: &[T],
    _focal_unit: u64,
    _init_cluster: &[usize],
    _config: &Config,
) -> (Vec<usize>, f64) {
    unimplemented!()
    // // First, making the mapping from chunks -> usize, and units -> usize.
    // let reads = map_unit_and_clusters(reads);
    // let k = config.cluster_num;
    // let mut model = RawModel::new(&reads, &weights, k);
    // // let mut unit_counts: HashMap<_, usize> = HashMap::new();
    // // for read in reads.iter() {
    // //     for node in read.nodes.iter() {
    // //         *unit_counts.entry(node.unit).or_default() += 1;
    // //     }
    // // }
    // // let use_units: HashSet<_> = unit_counts
    // //     .iter()
    // //     .filter(|&(_, &c)| c > config.coverage_thr)
    // //     .map(|(&x, _)| x)
    // //     .collect();
    // let mut diff = 10.;
    // let mut lk = reads.iter().map(|read| model.lk(read)).sum::<f64>();
    // trace!("LK:{}", lk);
    // while diff > 0.00000001 {
    //     weights = reads.iter().map(|read| model.weight(read)).collect();
    //     model = RawModel::new(&reads, &weights, k);
    //     let new_lk = reads.iter().map(|read| model.lk(read)).sum::<f64>();
    //     trace!("LK:{}", lk);
    //     diff = new_lk - lk;
    //     assert!(diff > -0.01, "{}", diff);
    //     lk = new_lk;
    // }
    // for (idx, w) in weights.iter().enumerate() {
    //     let w: Vec<_> = w.iter().map(|x| format!("{:.1}", x)).collect();
    //     let lk = model.lk(&reads[idx]);
    //     trace!("WEIGHT\t{}\t{}\t{}", idx, w.join("\t"), lk);
    // }
    // let predictions: Vec<_> = weights
    //     .iter()
    //     .map(|ws| {
    //         ws.iter()
    //             .enumerate()
    //             .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
    //             .map(|x| x.0)
    //             .unwrap()
    //     })
    //     .collect();
    // (predictions, lk)
}

fn em_clustering_old(
    units: &[Unit],
    reads: &[&EncodedRead],
    mut weights: Vec<Vec<f64>>,
    config: &Config,
) -> (Vec<usize>, f64) {
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
    let k = config.cluster_num;
    let mut model = Model::new(units, reads, &weights, k, &use_units);
    let mut diff = 10.;
    let mut lk = reads.iter().map(|read| model.lk(read)).sum::<f64>();
    trace!("LK:{}", lk);
    while diff > 0.00000001 {
        weights = reads.iter().map(|read| model.weight(read)).collect();
        model = Model::new(units, reads, &weights, k, &use_units);
        let new_lk = reads.iter().map(|read| model.lk(read)).sum::<f64>();
        trace!("LK:{}", lk);
        diff = new_lk - lk;
        assert!(diff > -0.01, "{}", diff);
        lk = new_lk;
    }
    for (idx, w) in weights.iter().enumerate() {
        let w: Vec<_> = w.iter().map(|x| format!("{:.1}", x)).collect();
        let lk = model.lk(&reads[idx]);
        trace!("WEIGHT\t{}\t{}\t{}", idx, w.join("\t"), lk);
    }
    let predictions: Vec<_> = weights
        .iter()
        .map(|ws| {
            ws.iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .map(|x| x.0)
                .unwrap()
        })
        .collect();
    (predictions, lk)
}

struct Model {
    // Fraction for each cluster.
    fraction: Vec<f64>,
    // Cluster -> Unit -> Category
    // For all i in 0..cluster_num, j in units,
    // category_fraction[i][j].iter().sum::<f64>() == 1 holds.
    category_fraction: Vec<HashMap<u64, Vec<f64>>>,
}
const SMALL_WEIGHT: f64 = 0.00000001;

impl Model {
    fn new(
        units: &[Unit],
        reads: &[&EncodedRead],
        weights: &[Vec<f64>],
        k: usize,
        use_units: &HashSet<u64>,
    ) -> Self {
        let category_num: HashMap<_, _> = units
            .iter()
            .filter(|u| use_units.contains(&u.id))
            .map(|u| (u.id, u.cluster_num))
            .collect();
        assert!(weights.iter().all(|ws| ws.len() == k));
        let total_weights: Vec<_> = (0..k)
            .map(|cluster| weights.iter().map(|ws| ws[cluster]).sum::<f64>())
            .collect();
        assert!((reads.len() as f64 - total_weights.iter().sum::<f64>()).abs() < 0.001);
        let fraction: Vec<_> = total_weights
            .iter()
            .map(|f| f / weights.len() as f64 + SMALL_WEIGHT)
            .collect();
        let sums_to_one = (1. - fraction.iter().sum::<f64>()).abs() < 0.0001;
        assert!(
            sums_to_one,
            "{:?}\t{}\t{}",
            fraction,
            weights.len(),
            reads.len(),
        );
        let mut category_fraction: Vec<HashMap<_, Vec<f64>>> = (0..k)
            .map(|_| {
                category_num
                    .iter()
                    .map(|(&unit_id, &category_num)| {
                        let slots = vec![SMALL_WEIGHT; category_num + 1];
                        (unit_id, slots)
                    })
                    .collect()
            })
            .collect();
        for (read, weight) in reads.iter().zip(weights.iter()) {
            for (cluster, w) in weight.iter().enumerate() {
                for node in read.nodes.iter() {
                    let (d, m) = (node.unit, node.cluster as usize);
                    category_fraction[cluster]
                        .entry(d)
                        .and_modify(|slots| slots[m] += w);
                }
            }
        }
        for clusterwise_weight in category_fraction.iter_mut() {
            for position_fraction in clusterwise_weight.values_mut() {
                let sum = position_fraction.iter().sum::<f64>();
                position_fraction.iter_mut().for_each(|x| *x /= sum);
            }
        }
        for xs in category_fraction.iter() {
            for x in xs.values() {
                assert!((1. - x.iter().sum::<f64>()).abs() < 0.001);
            }
        }
        Self {
            category_fraction,
            fraction,
        }
    }
    fn weight(&self, read: &EncodedRead) -> Vec<f64> {
        let log_weight: Vec<_> = self
            .fraction
            .iter()
            .zip(self.category_fraction.iter())
            .map(|(f, cluster)| {
                let read_lk = read
                    .nodes
                    .iter()
                    .filter_map(|node| cluster.get(&node.unit).map(|xs| xs[node.cluster as usize]))
                    .map(|x| x.ln())
                    .sum::<f64>();
                read_lk + f.ln()
            })
            .collect();
        let log_total_weight = logsumexp(&log_weight);
        let weight: Vec<_> = log_weight
            .iter()
            .map(|x| (x - log_total_weight).exp())
            .collect();
        assert!((1. - weight.iter().sum::<f64>()).abs() < 0.001);
        weight
    }
    fn lk(&self, read: &EncodedRead) -> f64 {
        let log_lks: Vec<_> = self
            .fraction
            .iter()
            .zip(self.category_fraction.iter())
            .map(|(f, cluster)| {
                let read_lk = read
                    .nodes
                    .iter()
                    .filter_map(|node| cluster.get(&node.unit).map(|xs| xs[node.cluster as usize]))
                    .map(|x| x.ln())
                    .sum::<f64>();
                read_lk + f.ln()
            })
            .collect();
        logsumexp(&log_lks)
    }
}

fn logsumexp(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.;
    }
    let max = xs.iter().max_by(|x, y| x.partial_cmp(&y).unwrap()).unwrap();
    let sum = xs.iter().map(|x| (x - max).exp()).sum::<f64>().ln();
    assert!(sum >= 0., "{:?}->{}", xs, sum);
    max + sum
}

// struct RawModel {
//     fraction: Vec<f64>,
//     category_fraction: Vec<Vec<Vec<f64>>>,
// }

// impl RawModel {
//     const PSEUDO_COUNT: f64 = 1f64;
//     fn new(reads: &[Vec<(usize, usize)>], weights: &[Vec<f64>], k: usize) -> Self {
//         let unit_num = reads
//             .iter()
//             .flat_map(|read| read.iter())
//             .map(|x| x.0 + 1)
//             .max()
//             .unwrap();
//         let category_num = {
//             let mut catnum = vec![1; unit_num];
//             for &(unit, cluster) in reads.iter().flat_map(|read| read.iter()) {
//                 catnum[unit] = catnum[unit].max(cluster + 1);
//             }
//             catnum
//         };
//         assert!(weights.iter().all(|ws| ws.len() == k));
//         let fraction: Vec<_> = {
//             let mut frac = vec![Self::PSEUDO_COUNT; k];
//             for ws in weights.iter() {
//                 for (f, w) in frac.iter_mut().zip(ws) {
//                     *f += w;
//                 }
//             }
//             let sum = frac.iter().sum::<f64>();
//             frac.iter_mut().for_each(|x| *x /= sum);
//             frac
//         };
//         let sums_to_one = (1. - fraction.iter().sum::<f64>()).abs() < 0.0001;
//         assert!(
//             sums_to_one,
//             "{:?}\t{}\t{}",
//             fraction,
//             weights.len(),
//             reads.len(),
//         );
//         let category_fraction = {
//             let mut catfrac: Vec<Vec<Vec<_>>> = (0..k)
//                 .map(|_| {
//                     category_num
//                         .iter()
//                         .map(|&x| vec![Self::PSEUDO_COUNT; x])
//                         .collect()
//                 })
//                 .collect();
//             for (read, ws) in reads.iter().zip(weights.iter()) {
//                 for (slots, w) in catfrac.iter_mut().zip(ws.iter()) {
//                     for &(unit, cluster) in read.iter() {
//                         slots[unit][cluster] += w;
//                     }
//                 }
//             }
//             catfrac.iter_mut().for_each(|component| {
//                 component.iter_mut().for_each(|position| {
//                     let sum = position.iter().sum::<f64>();
//                     position.iter_mut().for_each(|x| *x /= sum);
//                 })
//             });
//             catfrac
//         };
//         Self {
//             category_fraction,
//             fraction,
//         }
//     }
//     fn lk(&self, read: &[(usize, usize)]) -> f64 {
//         let lks: Vec<_> = self
//             .category_fraction
//             .iter()
//             .zip(self.fraction.iter())
//             .map(|(component, f)| {
//                 let prob = read.iter().map(|&(u, c)| component[u][c].ln()).sum::<f64>();
//                 f.ln() + prob
//             })
//             .collect();
//         logsumexp(&lks)
//     }
//     fn weight(&self, read: &[(usize, usize)]) -> Vec<f64> {
//         let mut lks: Vec<_> = self
//             .category_fraction
//             .iter()
//             .zip(self.fraction.iter())
//             .map(|(component, f)| {
//                 let prob = read.iter().map(|&(u, c)| component[u][c].ln()).sum::<f64>();
//                 f.ln() + prob
//             })
//             .collect();
//         let total = logsumexp(&lks);
//         lks.iter_mut().for_each(|x| {
//             *x = (*x - total).exp();
//         });
//         lks
//     }
// }

#[cfg(test)]
mod test {
    // use super::*;
    // use rand::Rng;
    // use rand::SeedableRng;
    // use rand_xoshiro::Xoshiro256StarStar;
    // #[test]
    // fn raw_clustering_test() {
    //     let template1: Vec<_> = (0..10).map(|x| (x, 0)).collect();
    //     let template2: Vec<_> = (0..10).map(|x| (x, 1)).collect();
    //     let reads: Vec<_> = (0..100)
    //         .map(|i| {
    //             if i < 50 {
    //                 template1.clone()
    //             } else {
    //                 template2.clone()
    //             }
    //         })
    //         .collect();
    //     let mut rng: Xoshiro256StarStar = SeedableRng::seed_from_u64(3948);
    //     let config = Config::new(10, 242, 2, 2, 2);
    //     let weights = (0..100)
    //         .map(|_| {
    //             let x = rng.gen_range(0f64..1f64);
    //             vec![x, 1f64 - x]
    //         })
    //         .collect();
    //     let (pred, _) = em_clustering_old(&reads, weights, &config);
    //     let answer0: Vec<_> = vec![vec![0; 50], vec![1; 50]].concat();
    //     let answer1: Vec<_> = vec![vec![1; 50], vec![0; 50]].concat();
    //     assert!(pred == answer1 || pred == answer0);
    // }
    // #[test]
    // fn context_clustering_test() {
    //     let template1: Vec<_> = (0..5).chain(10..15).map(|x| (x, 0)).collect();
    //     let template2: Vec<_> = (3..12).map(|x| (x, 0)).collect();
    //     let reads: Vec<_> = (0..100)
    //         .map(|i| {
    //             if i < 50 {
    //                 template1.clone()
    //             } else {
    //                 template2.clone()
    //             }
    //         })
    //         .collect();
    //     let mut rng: Xoshiro256StarStar = SeedableRng::seed_from_u64(42);
    //     let config = Config::new(10, 2424, 2, 2, 2);
    //     let weights = (0..100)
    //         .map(|_| {
    //             let x = rng.gen_range(0f64..1f64);
    //             vec![x, 1f64 - x]
    //         })
    //         .collect();
    //     for (i, read) in reads.iter().enumerate() {
    //         println!("{}\t{:?}", i, read);
    //     }
    //     let (pred, _) = em_clustering_old(&reads, weights, &config);
    //     println!("{:?}", pred);
    //     let answer0: Vec<_> = vec![vec![0; 50], vec![1; 50]].concat();
    //     let answer1: Vec<_> = vec![vec![1; 50], vec![0; 50]].concat();
    //     assert!(pred == answer1 || pred == answer0);
    // }
    // #[test]
    // fn random_check() {}
    // #[test]
    // fn random_check_context() {}
}
