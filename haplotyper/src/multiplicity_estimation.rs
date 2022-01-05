use crate::assemble::{ditch_graph::DitchGraph, *};
use definitions::DataSet;
use serde::*;
use std::collections::HashMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiplicityEstimationConfig {
    seed: u64,
    path: Option<String>,
    thread: usize,
}

impl MultiplicityEstimationConfig {
    pub fn new(thread: usize, seed: u64, path: Option<&str>) -> Self {
        Self {
            thread,
            seed,
            path: path.map(|x| x.to_string()),
        }
    }
}

pub trait MultiplicityEstimation {
    fn estimate_multiplicity(&mut self, config: &MultiplicityEstimationConfig);
}

impl MultiplicityEstimation for DataSet {
    fn estimate_multiplicity(&mut self, config: &MultiplicityEstimationConfig) {
        let cov = {
            let mut counts: HashMap<_, u32> = HashMap::new();
            for node in self.encoded_reads.iter().flat_map(|r| r.nodes.iter()) {
                *counts.entry(node.unit).or_default() += 1;
            }
            let mut counts: Vec<_> = counts.values().copied().collect();
            counts.sort_unstable();
            counts[counts.len() / 2] as f64 / 2f64
        };
        debug!("MULTP\tCOVERAGE\t{}\tHAPLOID", cov);
        self.coverage = Some(cov);
        let reads: Vec<_> = self.encoded_reads.iter().collect();
        let assemble_config = AssembleConfig::new(config.thread, 100, false, false, 4);
        let mut graph =
            ditch_graph::DitchGraph::new(&reads, Some(&self.selected_chunks), &assemble_config);
        graph.remove_lightweight_edges(2, true);
        let lens: Vec<_> = self.raw_reads.iter().map(|x| x.seq().len()).collect();
        // graph.assign_copy_number_gbs(cov, &lens);
        // graph.remove_zero_copy_elements(&lens, 0.3);
        graph.clean_up_graph_for_assemble(cov, &lens, &reads, &assemble_config);
        // let (nodes, _) = graph.copy_number_estimation_gbs(cov, &lens);
        let (nodes, _) = graph.copy_number_estimation(cov, &lens);
        let mut chunks: HashMap<_, _> =
            self.selected_chunks.iter_mut().map(|c| (c.id, c)).collect();
        // reset copy number.
        chunks.values_mut().for_each(|c| c.cluster_num = 0);
        for ((chunk, _), cp) in nodes.iter() {
            if let Some(c) = chunks.get_mut(chunk) {
                c.cluster_num += cp;
            }
        }
        // Rescue 0-copy number chunks.
        chunks
            .values_mut()
            .for_each(|c| c.cluster_num = c.cluster_num.max(1));
        {
            // DUMP Information
            let mut counts: HashMap<(u64, u64), usize> = HashMap::new();
            for node in self.encoded_reads.iter().flat_map(|r| r.nodes.iter()) {
                *counts.entry((node.unit, node.cluster)).or_default() += 1;
            }
            let mut counts_group: HashMap<_, Vec<_>> = HashMap::new();
            for (node, cp) in nodes.iter() {
                let occ = counts.get(node).unwrap_or(&0);
                counts_group.entry(cp).or_default().push(*occ as usize);
            }
            let mut counts_group: Vec<_> = counts_group.into_iter().collect();
            counts_group.sort_by_key(|x| x.0);
            debug!("MULTP\tCopyNum\tOccs\tMean\tSD");
            for (cp, occs) in counts_group {
                let sum: usize = occs.iter().sum();
                let sumsq: usize = occs.iter().map(|x| x * x).sum();
                let mean = sum as f64 / occs.len() as f64;
                let sd = (sumsq as f64 / occs.len() as f64 - mean * mean).sqrt();
                debug!("MULTP\t{}\t{}\t{:.2}\t{:.2}", cp, occs.len(), mean, sd);
            }
        }
        if let Some(mut file) = config
            .path
            .as_ref()
            .and_then(|path| std::fs::File::create(path).ok())
            .map(std::io::BufWriter::new)
        {
            use std::io::Write;
            let gfa = convert_to_gfa(&graph, &assemble_config);
            writeln!(&mut file, "{}", gfa).unwrap();
        }
    }
}

fn convert_to_gfa(graph: &DitchGraph, c: &AssembleConfig) -> gfa::GFA {
    let (segments, edge, group, summaries) = graph.spell(c);
    let total_base = segments.iter().map(|x| x.slen).sum::<u64>();
    debug!("MULTIP\tAssembly\t{}\t{}bp", segments.len(), total_base);
    let nodes = segments.into_iter().map(|node| {
        let tags = summaries
            .iter()
            .find(|x| x.id == node.sid)
            .map(|contigsummary| {
                let total: usize = contigsummary.summary.iter().map(|n| n.occ).sum();
                let coverage =
                    gfa::SamTag::new(format!("cv:i:{}", total / contigsummary.summary.len()));
                let (cp, cpnum) = contigsummary
                    .summary
                    .iter()
                    .filter_map(|elm| elm.copy_number)
                    .fold((0, 0), |(cp, num), x| (cp + x, num + 1));
                let mut tags = vec![coverage];
                if cpnum != 0 {
                    tags.push(gfa::SamTag::new(format!("cp:i:{}", cp / cpnum)));
                }
                tags
            })
            .unwrap_or_else(Vec::new);
        gfa::Record::from_contents(gfa::Content::Seg(node), tags)
    });
    {
        for summary in summaries.iter() {
            let (copy_num, tig_num) = summary
                .summary
                .iter()
                .filter_map(|s| s.copy_number)
                .fold((0, 0), |(c, x), copynum| (c + copynum, x + 1));
            let copy_num = match tig_num {
                0 => 0,
                _ => (copy_num as f64 / tig_num as f64).round() as usize,
            };
            let ids: Vec<_> = summary
                .summary
                .iter()
                .map(|elm| format!("{}-{}", elm.unit, elm.cluster))
                .collect();
            debug!(
                "MULTIP\tContig\t{}\t{}\t{}",
                summary.id,
                copy_num,
                ids.join("\t")
            );
        }
    }
    let edges = edge
        .into_iter()
        .map(|(edge, tags)| gfa::Record::from_contents(gfa::Content::Edge(edge), tags));
    let group = gfa::Record::from_contents(gfa::Content::Group(group), vec![]);
    let group = std::iter::once(group);
    let header = gfa::Content::Header(gfa::Header::default());
    let header = std::iter::once(gfa::Record::from_contents(header, vec![]));
    let records: Vec<_> = header.chain(group).chain(nodes).chain(edges).collect();
    gfa::GFA::from_records(records)
}
