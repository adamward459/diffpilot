use crate::git;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Refs {
    pub checked_out: Option<String>,
    pub detected_base: String,
    pub local: Vec<String>,
    pub remote: Vec<String>,
    pub tags: Vec<String>,
    pub commits: Vec<Commit>,
}

impl Refs {
    pub fn suggested(&self) -> Vec<String> {
        let base = &self.detected_base;
        let short = base.split_once('/').map_or(base.as_str(), |(_, rest)| rest);
        let mut suggested = vec![base.clone()];
        if short != base && self.local.iter().any(|name| name == short) {
            suggested.push(short.to_string());
        }
        suggested
    }

    pub fn contains(&self, name: &str) -> bool {
        [&self.local, &self.remote, &self.tags]
            .iter()
            .any(|group| group.iter().any(|entry| entry == name))
            || self.commits.iter().any(|commit| commit.sha == name)
    }

    pub fn load(repo: &Path) -> Refs {
        let run = |args: &[&str]| git::run(repo, args).unwrap_or_default();
        Refs::parse(
            &run(&[
                "for-each-ref",
                "--sort=-committerdate",
                "--format=%(refname)",
                "refs/heads",
                "refs/remotes",
                "refs/tags",
            ]),
            &run(&["symbolic-ref", "-q", "--short", "HEAD"]),
            &run(&["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"]),
            &run(&["log", "-n", "10", "--format=%h%x09%s"]),
        )
    }

    pub fn parse(refs: &str, head: &str, origin_head: &str, log: &str) -> Refs {
        let mut local = Vec::new();
        let mut remote = Vec::new();
        let mut tags = Vec::new();
        for line in refs.split('\n').filter(|line| !line.is_empty()) {
            if let Some(name) = line.strip_prefix("refs/heads/") {
                local.push(name.to_string());
            } else if let Some(name) = line.strip_prefix("refs/remotes/") {
                if !name.ends_with("/HEAD") {
                    remote.push(name.to_string());
                }
            } else if let Some(name) = line.strip_prefix("refs/tags/") {
                tags.push(name.to_string());
            }
        }
        let commits = log
            .split('\n')
            .filter(|line| !line.is_empty())
            .filter_map(|line| {
                let (sha, subject) = line.split_once('\t').unwrap_or((line, ""));
                (!sha.is_empty()).then(|| Commit {
                    sha: sha.to_string(),
                    subject: subject.to_string(),
                })
            })
            .collect();
        let checked_out = non_empty(head);
        let detected_base = non_empty(origin_head)
            .or_else(|| {
                ["main", "master"]
                    .into_iter()
                    .find(|name| local.iter().any(|entry| entry == name))
                    .map(String::from)
            })
            .or_else(|| {
                local
                    .iter()
                    .find(|name| Some(name.as_str()) != checked_out.as_deref())
                    .cloned()
            })
            .unwrap_or_else(|| "HEAD".to_string());
        let local = local
            .into_iter()
            .filter(|name| Some(name.as_str()) != checked_out.as_deref())
            .collect();
        Refs {
            checked_out,
            detected_base,
            local,
            remote,
            tags,
            commits,
        }
    }
}

fn non_empty(text: &str) -> Option<String> {
    let value = text.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_origin_head_then_main_then_master() {
        let refs = "refs/heads/feature\nrefs/heads/master\nrefs/remotes/origin/HEAD\nrefs/remotes/origin/main\n";
        assert_eq!(
            Refs::parse(refs, "feature\n", "origin/main\n", "").detected_base,
            "origin/main"
        );
        assert_eq!(Refs::parse(refs, "feature\n", "", "").detected_base, "master");
    }

    #[test]
    fn groups_refs_and_hides_the_checked_out_branch_and_origin_head() {
        let refs = Refs::parse(
            "refs/heads/feature\nrefs/heads/main\nrefs/remotes/origin/HEAD\nrefs/remotes/origin/main\nrefs/tags/v1\n",
            "feature\n",
            "origin/main\n",
            "abc1234\tfeat: one\ndef5678\t\n",
        );
        assert_eq!(refs.checked_out.as_deref(), Some("feature"));
        assert_eq!(refs.local, ["main"]);
        assert_eq!(refs.remote, ["origin/main"]);
        assert_eq!(refs.tags, ["v1"]);
        assert_eq!(
            refs.commits,
            [
                Commit {
                    sha: "abc1234".into(),
                    subject: "feat: one".into()
                },
                Commit {
                    sha: "def5678".into(),
                    subject: "".into()
                },
            ]
        );
        assert_eq!(refs.suggested(), ["origin/main", "main"]);
    }

    #[test]
    fn detached_head_has_no_checked_out_branch() {
        let refs = Refs::parse("refs/heads/main\n", "", "", "");
        assert_eq!(refs.checked_out, None);
        assert_eq!(refs.detected_base, "main");
    }
}
