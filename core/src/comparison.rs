use crate::git;
use crate::paths::standardized;
use crate::refs::Refs;
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Comparison {
    #[serde(rename_all = "camelCase")]
    CheckedOut {
        base: String,
        includes_uncommitted: bool,
    },
    WorkingTree,
    Staged,
    Branch {
        branch: String,
        base: String,
    },
}

impl Comparison {
    pub fn checked_out(base: &str, includes_uncommitted: bool) -> Self {
        Self::CheckedOut {
            base: base.to_string(),
            includes_uncommitted,
        }
    }

    pub fn branch(branch: &str, base: &str) -> Self {
        Self::Branch {
            branch: branch.to_string(),
            base: base.to_string(),
        }
    }

    pub fn revisions(&self) -> Vec<String> {
        let words: Vec<&str> = match self {
            Self::CheckedOut {
                base,
                includes_uncommitted: true,
            } => vec!["--merge-base", base],
            Self::CheckedOut {
                base,
                includes_uncommitted: false,
            } => vec!["--merge-base", base, "HEAD"],
            Self::WorkingTree => vec!["HEAD"],
            Self::Staged => vec!["--cached"],
            Self::Branch { branch, base } => vec!["--merge-base", base, branch],
        };
        words.into_iter().map(String::from).collect()
    }

    pub fn base(&self) -> Option<&str> {
        match self {
            Self::CheckedOut { base, .. } | Self::Branch { base, .. } => Some(base),
            Self::WorkingTree | Self::Staged => None,
        }
    }

    pub fn branch_name(&self) -> Option<&str> {
        match self {
            Self::Branch { branch, .. } => Some(branch),
            _ => None,
        }
    }

    pub fn includes_uncommitted(&self) -> bool {
        match self {
            Self::CheckedOut {
                includes_uncommitted, ..
            } => *includes_uncommitted,
            Self::WorkingTree | Self::Staged => true,
            Self::Branch { .. } => false,
        }
    }

    pub fn refs(&self) -> Vec<&str> {
        self.base().into_iter().chain(self.branch_name()).collect()
    }

    pub fn with_branch(&self, branch: Option<&str>, default_base: &str) -> Self {
        let base = self.base().unwrap_or(default_base);
        match branch {
            Some(branch) => Self::branch(branch, base),
            None => Self::checked_out(base, true),
        }
    }

    pub fn with_base(&self, base: &str) -> Self {
        match self {
            Self::Branch { branch, .. } => Self::branch(branch, base),
            Self::CheckedOut {
                includes_uncommitted, ..
            } => Self::checked_out(base, *includes_uncommitted),
            Self::WorkingTree | Self::Staged => Self::checked_out(base, true),
        }
    }

    pub fn with_uncommitted(&self, includes: bool, default_base: &str) -> Self {
        if self.branch_name().is_some() {
            return self.clone();
        }
        Self::checked_out(self.base().unwrap_or(default_base), includes)
    }
}

pub fn store_key(repo: &Path, checked_out: Option<&str>) -> String {
    format!(
        "{}\n{}",
        standardized(repo).to_string_lossy(),
        checked_out.unwrap_or("HEAD")
    )
}

pub fn load_comparison(repo: &Path, settings: &Settings) -> (Refs, Comparison) {
    let refs = Refs::load(repo);
    let saved = settings.comparison(&store_key(repo, refs.checked_out.as_deref()));
    let restored = saved.filter(|saved| {
        saved
            .refs()
            .iter()
            .all(|name| refs.contains(name) || resolves(repo, name))
    });
    let comparison = restored.unwrap_or_else(|| Comparison::checked_out(&refs.detected_base, true));
    (refs, comparison)
}

/// Saves `next` and returns it, or returns `current` when nothing changed.
pub fn select_comparison(
    settings: &Settings,
    repo: &Path,
    refs: &Refs,
    current: &Comparison,
    next: Comparison,
) -> Comparison {
    if &next == current {
        return next;
    }
    settings.set_comparison(&store_key(repo, refs.checked_out.as_deref()), &next);
    next
}

fn resolves(repo: &Path, name: &str) -> bool {
    git::run(repo, &["rev-parse", "--verify", "-q", &format!("{name}^{{commit}}")]).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes;
    use crate::testutil::{commit, git as run_git, init_repo, write};
    use tempfile::TempDir;

    fn fixture() -> TempDir {
        let dir = init_repo();
        let repo = dir.path();
        write(repo, "base.txt", "base\n");
        commit(repo, "base");
        run_git(repo, &["checkout", "-q", "-b", "other"]);
        write(repo, "other.txt", "other\n");
        commit(repo, "other");
        run_git(repo, &["checkout", "-q", "main"]);
        run_git(repo, &["checkout", "-q", "-b", "feature"]);
        write(repo, "committed.txt", "committed\n");
        commit(repo, "committed");
        write(repo, "staged.txt", "staged\n");
        run_git(repo, &["add", "staged.txt"]);
        write(repo, "base.txt", "base\nedited\n");
        write(repo, ".gitignore", "ignored.txt\n");
        write(repo, "ignored.txt", "ignored\n");
        write(repo, "untracked.txt", "untracked\n");
        dir
    }

    fn paths(repo: &Path, comparison: &Comparison) -> Vec<String> {
        changes::load(repo, comparison)
            .unwrap()
            .into_iter()
            .map(|change| change.path)
            .collect()
    }

    #[test]
    fn shows_the_files_of_each_comparison() {
        let dir = fixture();
        let cases = [
            (
                Comparison::checked_out("main", true),
                vec![".gitignore", "base.txt", "committed.txt", "staged.txt", "untracked.txt"],
            ),
            (Comparison::checked_out("main", false), vec!["committed.txt"]),
            (
                Comparison::WorkingTree,
                vec![".gitignore", "base.txt", "staged.txt", "untracked.txt"],
            ),
            (Comparison::Staged, vec!["staged.txt"]),
            (Comparison::branch("other", "main"), vec!["other.txt"]),
            (Comparison::checked_out("other", false), vec!["committed.txt"]),
        ];
        for (comparison, expected) in cases {
            assert_eq!(paths(dir.path(), &comparison), expected, "{comparison:?}");
        }
    }

    #[test]
    fn marks_uncommitted_files_only_when_they_are_included() {
        let dir = fixture();
        let all = changes::load(dir.path(), &Comparison::checked_out("main", true)).unwrap();
        let uncommitted: Vec<_> = all.iter().filter(|c| c.is_uncommitted).map(|c| &c.path).collect();
        assert_eq!(uncommitted, [".gitignore", "base.txt", "staged.txt", "untracked.txt"]);
        let branch = changes::load(dir.path(), &Comparison::branch("feature", "main")).unwrap();
        assert!(branch.iter().all(|change| !change.is_uncommitted));
    }

    #[test]
    fn untracked_files_show_as_added_without_touching_the_index() {
        let dir = fixture();
        let index_before = std::fs::read(dir.path().join(".git/index")).unwrap();
        let all = changes::load(dir.path(), &Comparison::checked_out("main", true)).unwrap();
        let untracked = all.iter().find(|change| change.path == "untracked.txt").unwrap();
        assert_eq!(
            (untracked.status, untracked.additions),
            (crate::changes::FileStatus::Added, 1)
        );
        let files = crate::diff::load(dir.path(), &Comparison::WorkingTree).unwrap();
        assert!(files.iter().any(|file| file.path == "untracked.txt"));
        assert_eq!(std::fs::read(dir.path().join(".git/index")).unwrap(), index_before);
    }

    #[test]
    fn defaults_to_the_detected_base_with_uncommitted_changes() {
        let dir = fixture();
        let (refs, comparison) = load_comparison(dir.path(), &Settings::in_memory());
        assert_eq!(refs.checked_out.as_deref(), Some("feature"));
        let mut local = refs.local.clone();
        local.sort();
        assert_eq!(local, ["main", "other"]);
        assert_eq!(comparison, Comparison::checked_out("main", true));
    }

    #[test]
    fn restores_the_choice_for_the_same_checked_out_branch() {
        let dir = fixture();
        let repo = dir.path();
        let settings = Settings::in_memory();
        let (refs, first) = load_comparison(repo, &settings);
        select_comparison(&settings, repo, &refs, &first, Comparison::branch("other", "main"));

        assert_eq!(load_comparison(repo, &settings).1, Comparison::branch("other", "main"));

        run_git(repo, &["checkout", "-q", "main"]);
        assert_eq!(
            load_comparison(repo, &settings).1,
            Comparison::checked_out("main", true)
        );
    }

    #[test]
    fn falls_back_to_the_default_when_the_saved_ref_is_gone() {
        let dir = fixture();
        let repo = dir.path();
        let settings = Settings::in_memory();
        let (refs, first) = load_comparison(repo, &settings);
        select_comparison(&settings, repo, &refs, &first, Comparison::checked_out("other", false));
        run_git(repo, &["branch", "-q", "-D", "other"]);
        assert_eq!(
            load_comparison(repo, &settings).1,
            Comparison::checked_out("main", true)
        );
    }

    #[test]
    fn restores_a_commit_base_outside_the_recent_list() {
        let dir = fixture();
        let repo = dir.path();
        let sha = run_git(repo, &["rev-parse", "--short", "main"]).trim().to_string();
        let settings = Settings::in_memory();
        settings.set_comparison(&store_key(repo, Some("feature")), &Comparison::checked_out(&sha, true));
        assert_eq!(load_comparison(repo, &settings).1, Comparison::checked_out(&sha, true));
    }

    #[test]
    fn selecting_the_current_comparison_saves_nothing() {
        let dir = fixture();
        let settings = Settings::in_memory();
        let (refs, current) = load_comparison(dir.path(), &settings);
        select_comparison(&settings, dir.path(), &refs, &current, current.clone());
        assert_eq!(settings.comparison(&store_key(dir.path(), Some("feature"))), None);
    }

    #[test]
    fn revisions_follow_the_comparison() {
        assert_eq!(
            Comparison::checked_out("main", true).revisions(),
            ["--merge-base", "main"]
        );
        assert_eq!(
            Comparison::checked_out("main", false).revisions(),
            ["--merge-base", "main", "HEAD"]
        );
        assert_eq!(Comparison::WorkingTree.revisions(), ["HEAD"]);
        assert_eq!(Comparison::Staged.revisions(), ["--cached"]);
        assert_eq!(
            Comparison::branch("topic", "main").revisions(),
            ["--merge-base", "main", "topic"]
        );
    }

    #[test]
    fn edits_keep_what_still_applies() {
        let branch = Comparison::branch("topic", "main");
        assert_eq!(branch.with_base("dev"), Comparison::branch("topic", "dev"));
        assert_eq!(branch.with_branch(None, "x"), Comparison::checked_out("main", true));
        assert_eq!(
            Comparison::Staged.with_branch(Some("topic"), "dev"),
            Comparison::branch("topic", "dev")
        );
        assert_eq!(branch.with_uncommitted(true, "x"), branch);
        assert_eq!(
            Comparison::WorkingTree.with_uncommitted(false, "dev"),
            Comparison::checked_out("dev", false)
        );
    }

    #[test]
    fn round_trips_through_json() {
        for comparison in [
            Comparison::checked_out("main", false),
            Comparison::WorkingTree,
            Comparison::Staged,
            Comparison::branch("a", "b"),
        ] {
            let json = serde_json::to_string(&comparison).unwrap();
            assert_eq!(serde_json::from_str::<Comparison>(&json).unwrap(), comparison);
        }
    }
}
