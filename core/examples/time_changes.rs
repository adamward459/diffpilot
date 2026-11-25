use diffpilot_core::{changes, comparison::Comparison};
use std::time::Instant;

fn main() {
    let repo = std::path::PathBuf::from(std::env::args().nth(1).expect("repo path"));
    let comparison = Comparison::checked_out("main", true);
    let mut samples: Vec<f64> = (0..9)
        .map(|_| {
            let start = Instant::now();
            let files = changes::load(&repo, &comparison).unwrap();
            assert!(!files.is_empty());
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    println!(
        "changes::load median {:.1} ms (min {:.1}, max {:.1})",
        samples[4], samples[0], samples[8]
    );
}
