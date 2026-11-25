use crate::git::{self, GitError};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub(crate) struct Captured {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Captured {
    pub fn text(&self) -> String {
        match (self.stdout.trim().is_empty(), self.stderr.trim().is_empty()) {
            (false, false) => format!("{}\n{}", self.stdout.trim_end(), self.stderr.trim_end()),
            (false, true) => self.stdout.trim_end().to_string(),
            _ => self.stderr.trim_end().to_string(),
        }
    }

    pub fn into_result(self) -> Result<String, GitError> {
        if self.ok {
            Ok(self.stdout)
        } else {
            Err(GitError::Failed(self.text()))
        }
    }
}

pub(crate) fn git(repo: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Captured, GitError> {
    let binary = git::binary().ok_or(GitError::NotFound)?;
    let mut command = Command::new(binary);
    command
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_EDITOR", "true")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| GitError::Failed(error.to_string()))?;
    let writer = stdin.and_then(|input| {
        let mut pipe = child.stdin.take()?;
        let input = input.to_vec();
        Some(std::thread::spawn(move || {
            let _ = pipe.write_all(&input);
        }))
    });
    let output = child
        .wait_with_output()
        .map_err(|error| GitError::Failed(error.to_string()))?;
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    Ok(Captured {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

pub(crate) fn git_ok(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    git(repo, args, None)?.into_result()
}

pub(crate) fn git_succeeds(repo: &Path, args: &[&str]) -> bool {
    git(repo, args, None).is_ok_and(|out| out.ok)
}
