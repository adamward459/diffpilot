use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

pub fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn init_repo() -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    git(dir.path(), &["init", "-q", "-b", "main"]);
    dir
}

pub fn write(repo: &Path, name: &str, text: &str) {
    std::fs::write(repo.join(name), text).expect("write file");
}

pub fn commit(repo: &Path, message: &str) {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
}
