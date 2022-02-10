#![allow(unused_imports)]
// const IS_MOCK: bool = false;
use definitions::*;
use std::collections::HashMap;
use std::io::BufReader;
fn main() -> std::io::Result<()> {
    env_logger::init();
    let xs = b"ACA";
    let ys = b"C";
    let tops = {
        let mode = edlib_sys::AlignMode::Infix;
        let task = edlib_sys::AlignTask::Alignment;
        let aln = edlib_sys::edlib_align(ys, xs, mode, task);
        let (start, end) = aln.locations.unwrap()[0];
        let end = end + 1;
        let mut ops = vec![kiley::bialignment::Op::Ins; start];
        ops.extend(aln.operations.unwrap().iter().map(|op| match op {
            0 => kiley::bialignment::Op::Mat,
            1 => kiley::bialignment::Op::Del,
            2 => kiley::bialignment::Op::Ins,
            3 => kiley::bialignment::Op::Mism,
            _ => unreachable!(),
        }));
        ops.extend(std::iter::repeat(kiley::bialignment::Op::Ins).take(xs.len() - end));
        ops
    };
    eprintln!("{:?}", tops);
    let (_, ops) = kiley::bialignment::edit_dist_slow_ops_semiglobal(ys, xs);
    eprintln!("{:?}", ops);
    // let args: Vec<_> = std::env::args().collect();
    // let ds: DataSet =
    //     serde_json::de::from_reader(BufReader::new(std::fs::File::open(&args[1]).unwrap()))
    //         .unwrap();
    // let chunks: HashMap<_, _> = ds.selected_chunks.iter().map(|c| (c.id, c)).collect();
    // println!("Unit\tCluster\tIdentity");
    // for node in ds.encoded_reads.iter().flat_map(|x| x.nodes.iter()) {
    //     let ref_chunk = chunks[&node.unit];
    //     let (_, aln, _) = node.recover(ref_chunk);
    //     let dist = aln.iter().filter(|&&x| x != b'|').count();
    //     let identity = 1f64 - dist as f64 / aln.len() as f64;
    //     let post: Vec<_> = node.posterior.iter().map(|p| format!("{:.3}", p)).collect();
    //     let (unit, cluster, _post) = (node.unit, node.cluster, post.join("\t"));
    //     println!("{}\t{}\t{}", unit, cluster, identity);
    // }
    // use haplotyper::purge_diverged::PurgeDivergent;
    // let config = haplotyper::purge_diverged::PurgeDivConfig::default();
    // ds.purge(&config);
    // let mut tail_counts: HashMap<_, Vec<_>> = HashMap::new();
    // for read in ds.encoded_reads.iter() {
    //     if let Some(head) = read.nodes.first() {
    //         tail_counts
    //             .entry((head.unit, head.is_forward))
    //             .or_default()
    //             .push(&read.leading_gap);
    //     }
    //     if let Some(tail) = read.nodes.last() {
    //         tail_counts
    //             .entry((tail.unit, !tail.is_forward))
    //             .or_default()
    //             .push(&read.trailing_gap);
    //     }
    // }
    // for ((unit, dir), labs) in tail_counts.iter() {
    //     if labs.len() > 1 {
    //         let sum: usize = labs.iter().map(|x| x.len()).sum();
    //         println!("{}\t{}\t{}\t{}", unit, dir, sum / labs.len(), labs.len());
    //     }
    // }

    // let unit1: u64 = args[2].parse().unwrap();
    // let unit2: u64 = args[3].parse().unwrap();
    // let (mut saw_unit1, mut saw_unit2) = (0, 0);
    // let mut saw_both = vec![];
    // for read in ds.encoded_reads.iter() {
    //     let check1 = read.nodes.iter().any(|n| n.unit == unit1);
    //     saw_unit1 += check1 as u32;
    //     let check2 = read.nodes.iter().any(|n| n.unit == unit2);
    //     saw_unit2 += check2 as u32;
    //     if check1 && check2 {
    //         saw_both.push(read);
    //     }
    // }
    // println!(
    //     "{}:{},{}:{},Both:{}",
    //     unit1,
    //     saw_unit1,
    //     unit2,
    //     saw_unit2,
    //     saw_both.len()
    // );
    // summarize(&ds, &saw_both, unit1, unit2);
    Ok(())
}

// fn summarize(ds: &DataSet, reads: &[&EncodedRead], unit1: u64, unit2: u64) {

//     let id2desc: HashMap<_, _> = ds.raw_reads.iter().map(|r| (r.id, &r.name)).collect();
//     let mut counts: HashMap<(u64, u64), [u32; 2]> = HashMap::new();
//     for read in reads.iter() {
//         let ans = match IS_MOCK {
//             true => id2desc[&read.id].contains("hapA") as usize,
//             false => id2desc[&read.id].contains("000251v2") as usize,
//         };
//         for node in read
//             .nodes
//             .iter()
//             .filter(|n| [unit1, unit2].contains(&n.unit))
//         {
//             counts.entry((node.unit, node.cluster)).or_default()[ans] += 1;
//         }
//     }
//     println!("Comp\tunit\tcluster\thap1\thap2");
//     for ((_, cl), counts) in counts.iter().filter(|&(&(unit, _), _)| unit == unit1) {
//         println!("Comp\t{}\t{}\t{}\t{}", unit1, cl, counts[0], counts[1]);
//     }
//     for ((_, cl), counts) in counts.iter().filter(|&(&(unit, _), _)| unit == unit2) {
//         println!("Comp\t{}\t{}\t{}\t{}", unit2, cl, counts[0], counts[1]);
//     }
//     println!("Conn\tunit1cl\tunit2cl\tcount");
//     let mut counts: HashMap<(u64, u64), u32> = HashMap::new();
//     for read in reads.iter() {
//         let cl1s = read
//             .nodes
//             .iter()
//             .filter_map(|n| (n.unit == unit1).then(|| n.cluster));
//         let cl2s = read
//             .nodes
//             .iter()
//             .filter_map(|n| (n.unit == unit2).then(|| n.cluster));
//         for cl1 in cl1s {
//             for cl2 in cl2s.clone() {
//                 *counts.entry((cl1, cl2)).or_default() += 1;
//             }
//         }
//     }
//     for ((cl1, cl2), count) in counts.iter() {
//         println!("Conn\t{}\t{}\t{}", cl1, cl2, count);
//     }
// }
