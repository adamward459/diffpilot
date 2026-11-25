use crate::comparison::Comparison;
use crate::git::{self, GitError};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FileStatus {
    #[serde(rename = "A")]
    Added,
    #[serde(rename = "M")]
    Modified,
    #[serde(rename = "D")]
    Deleted,
    #[serde(rename = "R")]
    Renamed,
}

impl FileStatus {
    pub fn code(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        match code {
            "A" => Some(Self::Added),
            "M" => Some(Self::Modified),
            "D" => Some(Self::Deleted),
            "R" => Some(Self::Renamed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileChange {
    pub path: String,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub is_uncommitted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChangeSummary {
    pub file_count: usize,
    pub additions: u32,
    pub deletions: u32,
    pub viewed: usize,
}

impl ChangeSummary {
    pub fn new(changes: &[FileChange], viewed: &HashSet<String>) -> Self {
        let mut summary = Self {
            file_count: changes.len(),
            ..Self::default()
        };
        for change in changes {
            summary.additions += change.additions;
            summary.deletions += change.deletions;
            if viewed.contains(&change.path) {
                summary.viewed += 1;
            }
        }
        summary
    }
}

pub fn load(repo: &Path, comparison: &Comparison) -> Result<Vec<FileChange>, GitError> {
    let revisions = comparison.revisions();
    let run = |base: &[&str]| -> Result<String, GitError> {
        let args: Vec<&str> = base
            .iter()
            .copied()
            .chain(revisions.iter().map(String::as_str))
            .collect();
        git::run(repo, &args)
    };
    let (name_status, numstat, uncommitted) = std::thread::scope(|scope| {
        let name_status = scope.spawn(|| run(&["diff", "-z", "-M", "--name-status"]));
        let numstat = scope.spawn(|| run(&["diff", "-z", "-M", "--numstat"]));
        let uncommitted = comparison
            .includes_uncommitted()
            .then(|| scope.spawn(|| git::run(repo, &["diff", "-z", "--name-only", "HEAD"])));
        let join = |handle: std::thread::ScopedJoinHandle<'_, _>| handle.join().expect("git thread panicked");
        (join(name_status), join(numstat), uncommitted.map(join))
    });
    Ok(parse(
        &name_status?,
        &numstat?,
        &uncommitted.transpose()?.unwrap_or_default(),
    ))
}

pub fn parse(name_status: &str, numstat: &str, uncommitted: &str) -> Vec<FileChange> {
    let mut statuses: BTreeMap<&str, FileStatus> = BTreeMap::new();
    let mut fields = name_status.split('\0');
    while let Some(code) = fields.next().filter(|code| !code.is_empty()) {
        let is_rename = code.starts_with('R');
        if is_rename {
            fields.next();
        }
        let Some(path) = fields.next() else { break };
        let status = if is_rename {
            FileStatus::Renamed
        } else {
            code.get(..1)
                .and_then(FileStatus::from_code)
                .unwrap_or(FileStatus::Modified)
        };
        statuses.insert(path, status);
    }

    let mut counts: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    let mut entries = numstat.split('\0');
    while let Some(entry) = entries.next().filter(|entry| !entry.is_empty()) {
        let parts: Vec<&str> = entry.splitn(3, '\t').collect();
        if parts.len() != 3 {
            break;
        }
        let mut path = parts[2];
        if path.is_empty() {
            entries.next();
            path = entries.next().unwrap_or("");
        }
        counts.insert(path, (parts[0].parse().unwrap_or(0), parts[1].parse().unwrap_or(0)));
    }

    let uncommitted: HashSet<&str> = uncommitted.split('\0').filter(|p| !p.is_empty()).collect();
    statuses
        .into_iter()
        .map(|(path, status)| {
            let (additions, deletions) = counts.get(path).copied().unwrap_or((0, 0));
            FileChange {
                path: path.to_string(),
                status,
                additions,
                deletions,
                is_uncommitted: uncommitted.contains(path),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, status: FileStatus, additions: u32, deletions: u32, uncommitted: bool) -> FileChange {
        FileChange {
            path: path.into(),
            status,
            additions,
            deletions,
            is_uncommitted: uncommitted,
        }
    }

    #[test]
    fn parses_statuses_counts_and_renames() {
        let changes = parse(
            "M\0src/a.ts\0A\0src/new.ts\0D\0old.md\0R087\0src/Old.ts\0src/Renamed.ts\0",
            "3\t1\tsrc/a.ts\0\x35\t0\tsrc/new.ts\0\x30\t4\told.md\0\x32\t2\t\0src/Old.ts\0src/Renamed.ts\0",
            "src/new.ts\0",
        );
        assert_eq!(
            changes,
            [
                change("old.md", FileStatus::Deleted, 0, 4, false),
                change("src/Renamed.ts", FileStatus::Renamed, 2, 2, false),
                change("src/a.ts", FileStatus::Modified, 3, 1, false),
                change("src/new.ts", FileStatus::Added, 5, 0, true),
            ]
        );
    }

    #[test]
    fn empty_diff_has_no_changes() {
        assert!(parse("", "", "").is_empty());
    }

    #[test]
    fn binary_files_count_as_zero_lines() {
        let changes = parse("M\0logo.png\0", "-\t-\tlogo.png\0", "");
        assert_eq!(changes, [change("logo.png", FileStatus::Modified, 0, 0, false)]);
    }

    #[test]
    fn summary_totals_lines_and_viewed_files() {
        let changes = [
            change("a", FileStatus::Modified, 3, 1, false),
            change("b", FileStatus::Added, 5, 0, false),
        ];
        let viewed = HashSet::from(["a".to_string(), "gone".to_string()]);
        assert_eq!(
            ChangeSummary::new(&changes, &viewed),
            ChangeSummary {
                file_count: 2,
                additions: 8,
                deletions: 1,
                viewed: 1
            }
        );
    }
}
