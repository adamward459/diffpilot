use std::path::Path;
use std::process::Command;

fn init(dir: &Path) {
    let status = Command::new("git").arg("init").arg("-q").arg(dir).status().unwrap();
    assert!(status.success());
}

#[test]
fn an_inherited_git_dir_does_not_redirect_git_run() {
    let repo = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    init(repo.path());
    init(other.path());
    // SAFETY: this binary has only this test, so no other thread reads the environment.
    unsafe {
        std::env::set_var("GIT_DIR", other.path().join(".git"));
        std::env::set_var("GIT_WORK_TREE", other.path());
    }

    let top = diffpilot_core::git::run(repo.path(), &["rev-parse", "--show-toplevel"]).unwrap();

    let expected = repo.path().canonicalize().unwrap();
    assert_eq!(Path::new(top.trim()).canonicalize().unwrap(), expected);
}
