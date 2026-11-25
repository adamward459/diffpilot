use crate::paths::find_executable;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitError {
    #[error("git was not found")]
    NotFound,
    #[error("{0}")]
    Failed(String),
}

// /usr/bin/git is an xcrun shim, and xcrun refuses to run inside the App Sandbox.
pub const CANDIDATES: [&str; 4] = [
    "/Applications/Xcode.app/Contents/Developer/usr/bin/git",
    "/Library/Developer/CommandLineTools/usr/bin/git",
    "/opt/homebrew/bin/git",
    "/usr/local/bin/git",
];

pub fn binary() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = CANDIDATES.iter().map(PathBuf::from).collect();
    find_executable(&candidates, "git")
}

pub fn run(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    let binary = binary().ok_or(GitError::NotFound)?;
    let output = Command::new(binary)
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| GitError::Failed(error.to_string()))?;
    if !output.status.success() {
        return Err(GitError::Failed(String::from_utf8_lossy(&output.stderr).into_owned()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::init_repo;

    #[test]
    fn runs_git_in_the_repo() {
        let repo = init_repo();
        let branch = run(repo.path(), &["symbolic-ref", "--short", "HEAD"]).unwrap();
        assert_eq!(branch.trim(), "main");
    }

    #[test]
    fn returns_gits_message_on_failure() {
        let repo = init_repo();
        match run(repo.path(), &["rev-parse", "no-such-ref"]) {
            Err(GitError::Failed(message)) => assert!(message.contains("no-such-ref")),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
