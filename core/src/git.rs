use crate::paths::find_executable;
use std::ffi::OsStr;
use std::io::Write;
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
    run_io(repo, args, &[], None).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Like `run`, with extra environment variables and bytes for stdin. Returns stdout as raw bytes.
pub fn run_io(repo: &Path, args: &[&str], env: &[(&str, &OsStr)], stdin: Option<&[u8]>) -> Result<Vec<u8>, GitError> {
    let binary = binary().ok_or(GitError::NotFound)?;
    let mut command = Command::new(binary);
    command
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    for (name, value) in env {
        command.env(name, value);
    }
    let mut child = command.spawn().map_err(|error| GitError::Failed(error.to_string()))?;
    let writer = stdin.map(|input| {
        let mut pipe = child.stdin.take().expect("stdin is piped");
        let input = input.to_vec();
        std::thread::spawn(move || {
            let _ = pipe.write_all(&input);
        })
    });
    let output = child
        .wait_with_output()
        .map_err(|error| GitError::Failed(error.to_string()))?;
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    if !output.status.success() {
        return Err(GitError::Failed(String::from_utf8_lossy(&output.stderr).into_owned()));
    }
    Ok(output.stdout)
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
