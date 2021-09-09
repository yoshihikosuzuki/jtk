use definitions::*;
use std::io::BufReader;
fn main() -> std::io::Result<()> {
    env_logger::init();
    let args: Vec<_> = std::env::args().collect();
    let ds: DataSet =
        serde_json::de::from_reader(BufReader::new(std::fs::File::open(&args[1]).unwrap()))
            .unwrap();
    use haplotyper::em_correction::*;
    let repeat_num = 40;
    let coverage_thr = 5;
    use std::collections::HashMap;
    let id2desc: HashMap<_, _> = ds
        .raw_reads
        .iter()
        .map(|read| (read.id, &read.desc))
        .collect();
    log::debug!("RESULT\tK\tLK\tCov");
    let unit_ids: Vec<u64> = args[2..].iter().map(|x| x.parse().unwrap()).collect();
    for unit_id in unit_ids {
        log::debug!("Clustering...{}", unit_id);
        let ref_unit = ds.selected_chunks.iter().find(|n| n.id == unit_id).unwrap();
        let reads: Vec<_> = ds
            .encoded_reads
            .iter()
            .filter(|r| r.nodes.iter().any(|n| n.unit == unit_id))
            .collect();
        let k = ref_unit.cluster_num;
        let (new_clustering, lk, cluster_num) = (1..=k)
            .flat_map(|k| {
                let len = if k == 1 { 1 } else { repeat_num };
                std::iter::repeat(k).take(len)
            })
            .enumerate()
            .map(|(i, k)| {
                let seed = unit_id * (i * k) as u64;
                let config = Config::new(repeat_num, seed, k, unit_id, true, coverage_thr);
                let (xs, lk, cn) = em_clustering(&reads, &config);
                log::debug!("INSPECT\t{}\t{}", lk / xs.len() as f64, seed);
                (xs, lk, cn)
            })
            .max_by(|x, y| (x.1).partial_cmp(&(y.1)).unwrap())
            .unwrap();
        let mean_lk = lk / new_clustering.len() as f64;
        log::debug!(
            "RESULT\t{}\t{}\t{}",
            cluster_num,
            mean_lk,
            new_clustering.len()
        );
        let reads: Vec<_> = reads
            .iter()
            .flat_map(|r| {
                let is_hapa = id2desc[&r.id].contains("252v2") as u8;
                r.nodes
                    .iter()
                    .filter(|n| n.unit == unit_id)
                    .map(|_| is_hapa)
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(reads.len(), new_clustering.len());
        for (r, cl) in reads.iter().zip(new_clustering.iter()) {
            log::debug!("ASN\t{}\t{}", r, cl.2);
        }
    }
    Ok(())
}
