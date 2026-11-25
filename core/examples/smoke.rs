use diffpilot_core::comparison::Comparison;
use diffpilot_core::{changes, diff, tree};
use std::collections::HashSet;
use std::path::Path;

fn main() {
    let repo = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let repo = Path::new(&repo);
    let comparison = Comparison::checked_out("main", true);
    let changes = changes::load(repo, &comparison).expect("changes load");
    let summary = changes::ChangeSummary::new(&changes, &HashSet::new());
    println!(
        "changes: {} files, +{} -{}",
        summary.file_count, summary.additions, summary.deletions
    );

    let root = tree::build(&changes);
    println!(
        "tree: {} top-level nodes, {} files",
        root.children.len(),
        tree::files(&root).len()
    );

    let files = diff::load(repo, &comparison).expect("diff loads");
    let hunks: usize = files.iter().map(|file| file.hunks.len()).sum();
    let lines: usize = files
        .iter()
        .flat_map(|file| &file.hunks)
        .map(|hunk| hunk.lines.len())
        .sum();
    println!("diff: {} files, {hunks} hunks, {lines} lines", files.len());
    if let Some(file) = files.first() {
        println!("first: {} fingerprint {}", file.path, &file.fingerprint()[..12]);
    }
}
