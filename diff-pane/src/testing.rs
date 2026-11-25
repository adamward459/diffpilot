use diffpilot_core::changes::FileStatus;
use diffpilot_core::diff::{DiffLine, FileDiff, Hunk, LineKind};

pub fn line(kind: LineKind, old: Option<u32>, new: Option<u32>, text: &str) -> DiffLine {
    DiffLine {
        kind,
        old,
        new,
        text: text.into(),
    }
}

pub fn added(new: u32, text: &str) -> DiffLine {
    line(LineKind::Added, None, Some(new), text)
}

pub fn diff(path: &str, lines: Vec<DiffLine>) -> FileDiff {
    diff_hunks(path, vec![lines])
}

pub fn diff_hunks(path: &str, hunks: Vec<Vec<DiffLine>>) -> FileDiff {
    let mut file = FileDiff::new(path, FileStatus::Modified);
    for lines in hunks {
        file.additions += lines.iter().filter(|l| l.kind == LineKind::Added).count() as u32;
        file.deletions += lines.iter().filter(|l| l.kind == LineKind::Deleted).count() as u32;
        file.hunks.push(Hunk {
            header: "@@ -1 +1 @@".into(),
            lines,
        });
    }
    file
}

/// One line per letter: `c` context, `d` deleted, `a` added, with line numbers that count up per side.
pub fn lines_with(spec: &str, text: &str) -> Vec<DiffLine> {
    let (mut old, mut new) = (0, 0);
    spec.chars()
        .map(|c| match c {
            'd' => {
                old += 1;
                line(LineKind::Deleted, Some(old), None, text)
            }
            'a' => {
                new += 1;
                line(LineKind::Added, None, Some(new), text)
            }
            _ => {
                old += 1;
                new += 1;
                line(LineKind::Context, Some(old), Some(new), text)
            }
        })
        .collect()
}

pub fn lines_of(spec: &str) -> Vec<DiffLine> {
    lines_with(spec, "x")
}
