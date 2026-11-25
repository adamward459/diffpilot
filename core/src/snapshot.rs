//! Snapshots of the working tree around an editing run, for "Show Change" and "Undo this run".
//!
//! A snapshot is a git tree of every file the working tree holds: tracked, modified, deleted and untracked,
//! but not ignored files. Nothing is stored outside git. The trees live in the repo's object database, and a
//! commit per tree under `refs/diffpilot/runs/<run>/` keeps them from being collected. Those refs are not
//! branches, stashes or tags, so no git command lists them by default.
//!
//! The tree is built in a temporary index: a copy of the real index, plus every changed or untracked file
//! hashed without clean filters (autocrlf, LFS), so a restore gives back the exact bytes. The real index and
//! the working tree are only read.

use crate::changes::{self, FileStatus};
use crate::diff::{self, FileDiff};
use crate::git::{self, GitError};
use crate::store::RunId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const REF_PREFIX: &str = "refs/diffpilot/runs";
const NULL_OID: &str = "0000000000000000000000000000000000000000";

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("Could not snapshot the working tree: {0}")]
    Git(#[from] GitError),
    #[error("Could not snapshot the working tree: {0}")]
    Io(#[from] std::io::Error),
    #[error("The run has not finished, so there is nothing to compare.")]
    Unfinished,
}

pub type Result<T> = std::result::Result<T, SnapshotError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub run: RunId,
    pub work_tree: String,
    pub index_tree: Option<String>,
    pub after_tree: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Until {
    RunEnd,
    Now,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UndoReport {
    pub restored: Vec<String>,
    pub removed: Vec<String>,
    /// Files the reviewer changed after the run. They stay as they are unless the undo is forced.
    pub conflicts: Vec<String>,
}

impl UndoReport {
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

fn toplevel(repo: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(text(&git::run_io(
        repo,
        &["rev-parse", "--show-toplevel"],
        &[],
        None,
    )?)))
}

fn real_index(top: &Path) -> Result<PathBuf> {
    let dir = text(&git::run_io(top, &["rev-parse", "--absolute-git-dir"], &[], None)?);
    Ok(PathBuf::from(dir).join("index"))
}

fn ref_name(run: RunId, which: &str) -> String {
    format!("{REF_PREFIX}/{}/{which}", run.0)
}

fn commit_tree(top: &Path, tree: &str, parent: Option<&str>, message: &str) -> Result<String> {
    let identity = [
        ("GIT_AUTHOR_NAME", "diffpilot"),
        ("GIT_AUTHOR_EMAIL", "diffpilot@localhost"),
        ("GIT_COMMITTER_NAME", "diffpilot"),
        ("GIT_COMMITTER_EMAIL", "diffpilot@localhost"),
    ];
    let env: Vec<(&str, &OsStr)> = identity
        .iter()
        .map(|(name, value)| (*name, OsStr::new(*value)))
        .collect();
    let mut args = vec!["-c", "commit.gpgsign=false", "commit-tree", tree, "-m", message];
    if let Some(parent) = parent {
        args.extend(["-p", parent]);
    }
    Ok(text(&git::run_io(top, &args, &env, None)?))
}

/// Hashes the working tree into a tree object without touching the real index.
fn tree_of_working_tree(repo: &Path) -> Result<(String, Option<String>)> {
    let top = toplevel(repo)?;
    let scratch = tempfile::tempdir()?;
    let index = scratch.path().join("index");
    let real = real_index(&top)?;
    if real.exists() {
        std::fs::copy(&real, &index)?;
    }
    let env = [("GIT_INDEX_FILE", index.as_os_str())];

    let index_tree = git::run_io(&top, &["write-tree"], &env, None)
        .ok()
        .map(|bytes| text(&bytes));

    let listed = git::run_io(
        &top,
        &["ls-files", "-z", "-m", "-d", "-o", "--exclude-standard"],
        &[],
        None,
    )?;
    let mut paths: Vec<String> = listed
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect();
    paths.sort();
    paths.dedup();

    let mut entries: Vec<(String, String, String)> = Vec::new();
    let mut files: Vec<(String, &'static str)> = Vec::new();
    for path in paths {
        match std::fs::symlink_metadata(top.join(&path)) {
            Err(_) => entries.push(("0".into(), NULL_OID.into(), path)),
            Ok(meta) if meta.is_dir() => {}
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = std::fs::read_link(top.join(&path))?;
                let oid = text(&git::run_io(
                    &top,
                    &["hash-object", "-w", "--stdin"],
                    &[],
                    Some(target.as_os_str().as_encoded_bytes()),
                )?);
                entries.push(("120000".into(), oid, path));
            }
            Ok(meta) => {
                let mode = if meta.permissions().mode() & 0o111 != 0 {
                    "100755"
                } else {
                    "100644"
                };
                files.push((path, mode));
            }
        }
    }
    let (plain, odd): (Vec<_>, Vec<_>) = files
        .into_iter()
        .partition(|(path, _)| !path.contains(['\n', '"', '\\']));
    if !plain.is_empty() {
        let input: String = plain.iter().map(|(path, _)| format!("{path}\n")).collect();
        let oids = text(&git::run_io(
            &top,
            &["hash-object", "-w", "--no-filters", "--stdin-paths"],
            &[],
            Some(input.as_bytes()),
        )?);
        for ((path, mode), oid) in plain.into_iter().zip(oids.lines()) {
            entries.push((mode.into(), oid.into(), path));
        }
    }
    for (path, mode) in odd {
        let oid = text(&git::run_io(
            &top,
            &["hash-object", "-w", "--no-filters", "--", &path],
            &[],
            None,
        )?);
        entries.push((mode.into(), oid, path));
    }
    if !entries.is_empty() {
        let mut input = Vec::new();
        for (mode, oid, path) in &entries {
            input.extend_from_slice(format!("{mode} {oid}\t{path}").as_bytes());
            input.push(0);
        }
        git::run_io(&top, &["update-index", "-z", "--index-info"], &env, Some(&input))?;
    }
    let tree = text(&git::run_io(&top, &["write-tree"], &env, None)?);
    Ok((tree, index_tree))
}

/// Call before an editing run starts.
pub fn take(repo: &Path, run: RunId) -> Result<Snapshot> {
    let top = toplevel(repo)?;
    let (work_tree, index_tree) = tree_of_working_tree(repo)?;
    let head = git::run_io(&top, &["rev-parse", "--verify", "-q", "HEAD"], &[], None)
        .ok()
        .map(|bytes| text(&bytes))
        .filter(|oid| !oid.is_empty());
    let index_commit = match &index_tree {
        Some(tree) => Some(commit_tree(
            &top,
            tree,
            head.as_deref(),
            "diffpilot: index before the run",
        )?),
        None => None,
    };
    let before = commit_tree(
        &top,
        &work_tree,
        index_commit.as_deref(),
        "diffpilot: working tree before the run",
    )?;
    git::run_io(&top, &["update-ref", &ref_name(run, "before"), &before], &[], None)?;
    Ok(Snapshot {
        run,
        work_tree,
        index_tree,
        after_tree: None,
    })
}

/// Call when the run ends, so the change stays comparable even if the reviewer edits files later.
pub fn finish(repo: &Path, snapshot: &mut Snapshot) -> Result<()> {
    let top = toplevel(repo)?;
    let (tree, _) = tree_of_working_tree(repo)?;
    let before = text(&git::run_io(
        &top,
        &["rev-parse", &ref_name(snapshot.run, "before")],
        &[],
        None,
    )?);
    let after = commit_tree(&top, &tree, Some(&before), "diffpilot: working tree after the run")?;
    git::run_io(
        &top,
        &["update-ref", &ref_name(snapshot.run, "after"), &after],
        &[],
        None,
    )?;
    snapshot.after_tree = Some(tree);
    Ok(())
}

/// Deletes the refs of a run. Its objects go with the next `git gc`.
pub fn forget(repo: &Path, run: RunId) -> Result<()> {
    let top = toplevel(repo)?;
    for which in ["before", "after"] {
        let name = ref_name(run, which);
        if git::run_io(&top, &["rev-parse", "--verify", "-q", &name], &[], None).is_ok() {
            git::run_io(&top, &["update-ref", "-d", &name], &[], None)?;
        }
    }
    Ok(())
}

fn end_tree(repo: &Path, snapshot: &Snapshot, until: Until) -> Result<String> {
    match until {
        Until::RunEnd => snapshot.after_tree.clone().ok_or(SnapshotError::Unfinished),
        Until::Now => Ok(tree_of_working_tree(repo)?.0),
    }
}

fn pathspec<'a>(mut args: Vec<&'a str>, paths: &'a [String]) -> Vec<&'a str> {
    if !paths.is_empty() {
        args.push("--");
        args.extend(paths.iter().map(String::as_str));
    }
    args
}

/// What the run changed, or what changed since the run started when `until` is `Now`.
pub fn changed_files(repo: &Path, snapshot: &Snapshot, until: Until) -> Result<Vec<ChangedFile>> {
    let after = end_tree(repo, snapshot, until)?;
    let top = toplevel(repo)?;
    let list = |mode: &str| -> Result<String> {
        let args = ["diff-tree", "-r", "-z", "-M", mode, &snapshot.work_tree, &after];
        Ok(String::from_utf8_lossy(&git::run_io(&top, &args, &[], None)?).into_owned())
    };
    Ok(changes::parse(&list("--name-status")?, &list("--numstat")?, "")
        .into_iter()
        .map(|change| ChangedFile {
            path: change.path,
            status: change.status,
            additions: change.additions,
            deletions: change.deletions,
        })
        .collect())
}

/// The diff from the snapshot, limited to `paths` when it is not empty.
pub fn diff(repo: &Path, snapshot: &Snapshot, until: Until, paths: &[String]) -> Result<Vec<FileDiff>> {
    let after = end_tree(repo, snapshot, until)?;
    let top = toplevel(repo)?;
    let mut args: Vec<&str> = diff::DIFF_ARGS.to_vec();
    args.extend([snapshot.work_tree.as_str(), after.as_str()]);
    let output = git::run_io(&top, &pathspec(args, paths), &[], None)?;
    Ok(diff::in_tree_order(diff::parse(&String::from_utf8_lossy(&output))))
}

struct Raw {
    old_mode: String,
    old_oid: String,
    status: char,
    path: String,
}

fn raw_diff(top: &Path, from: &str, to: &str) -> Result<Vec<Raw>> {
    let output = git::run_io(
        top,
        &["diff-tree", "-r", "-z", "--raw", "--no-renames", from, to],
        &[],
        None,
    )?;
    let mut fields = output.split(|byte| *byte == 0).filter(|field| !field.is_empty());
    let mut changes = Vec::new();
    while let (Some(header), Some(path)) = (fields.next(), fields.next()) {
        let header = String::from_utf8_lossy(header);
        let parts: Vec<&str> = header.trim_start_matches(':').split(' ').collect();
        if let [old_mode, _new_mode, old_oid, _new_oid, status] = parts.as_slice() {
            changes.push(Raw {
                old_mode: old_mode.to_string(),
                old_oid: old_oid.to_string(),
                status: status.chars().next().unwrap_or('M'),
                path: String::from_utf8_lossy(path).into_owned(),
            });
        }
    }
    Ok(changes)
}

fn paths_of(changes: &[Raw]) -> HashSet<&str> {
    changes.iter().map(|change| change.path.as_str()).collect()
}

fn write_back(top: &Path, change: &Raw) -> Result<bool> {
    let target = top.join(&change.path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::symlink_metadata(&target) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir(&target)?,
        Ok(_) => std::fs::remove_file(&target)?,
        Err(_) => {}
    }
    let content = git::run_io(top, &["cat-file", "blob", &change.old_oid], &[], None)?;
    match change.old_mode.as_str() {
        "120000" => {
            let link = String::from_utf8_lossy(&content).into_owned();
            std::os::unix::fs::symlink(link, &target)?;
        }
        "160000" => return Ok(false),
        mode => {
            std::fs::write(&target, content)?;
            let bits = if mode == "100755" { 0o755 } else { 0o644 };
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(bits))?;
        }
    }
    Ok(true)
}

fn remove_and_prune(top: &Path, path: &str) -> Result<()> {
    let target = top.join(path);
    match std::fs::symlink_metadata(&target) {
        Ok(meta) if meta.is_dir() => return Ok(()),
        Ok(_) => std::fs::remove_file(&target)?,
        Err(_) => {}
    }
    let mut dir = target.parent();
    while let Some(current) = dir.filter(|current| *current != top) {
        if std::fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
    Ok(())
}

/// Puts back every file the run changed. A file the reviewer changed after the run is a conflict and is left
/// alone unless `force` is set. Running it again changes nothing. The index entries of the restored files go
/// back to what they were before the run.
pub fn undo(repo: &Path, snapshot: &Snapshot, force: bool) -> Result<UndoReport> {
    let after = snapshot.after_tree.as_deref().ok_or(SnapshotError::Unfinished)?;
    let top = toplevel(repo)?;
    let (now, _) = tree_of_working_tree(repo)?;
    let run_changes = raw_diff(&top, &snapshot.work_tree, after)?;
    let since_before = raw_diff(&top, &snapshot.work_tree, &now)?;
    let since_run = raw_diff(&top, after, &now)?;
    let (differs_from_before, differs_from_after) = (paths_of(&since_before), paths_of(&since_run));

    let mut report = UndoReport::default();
    let mut to_restore = Vec::new();
    for change in &run_changes {
        if !differs_from_before.contains(change.path.as_str()) {
            continue;
        }
        if differs_from_after.contains(change.path.as_str()) && !force {
            report.conflicts.push(change.path.clone());
        } else {
            to_restore.push(change);
        }
    }
    for change in to_restore.iter().filter(|change| change.status == 'A') {
        remove_and_prune(&top, &change.path)?;
        report.removed.push(change.path.clone());
    }
    for change in to_restore.iter().filter(|change| change.status != 'A') {
        if write_back(&top, change)? {
            report.restored.push(change.path.clone());
        }
    }
    restore_index(&top, snapshot, &report)?;
    Ok(report)
}

fn restore_index(top: &Path, snapshot: &Snapshot, report: &UndoReport) -> Result<()> {
    let Some(index_tree) = &snapshot.index_tree else {
        return Ok(());
    };
    let touched: Vec<&String> = report.restored.iter().chain(&report.removed).collect();
    if touched.is_empty() {
        return Ok(());
    }
    let mut input = Vec::new();
    for path in touched {
        let args = ["ls-tree", "-z", index_tree.as_str(), "--", path.as_str()];
        let listed = git::run_io(top, &args, &[], None)?;
        let line = String::from_utf8_lossy(&listed);
        let line = line.trim_end_matches('\0');
        match line
            .split_once('\t')
            .map(|(meta, _)| meta.split(' ').collect::<Vec<_>>())
        {
            Some(meta) if meta.len() == 3 => {
                input.extend_from_slice(format!("{} {}\t{path}\0", meta[0], meta[2]).as_bytes())
            }
            _ => input.extend_from_slice(format!("0 {NULL_OID}\t{path}\0").as_bytes()),
        }
    }
    git::run_io(top, &["update-index", "-z", "--index-info"], &[], Some(&input))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{commit, git as run_git, init_repo, write};
    use std::os::unix::fs::symlink;

    const BINARY: &[u8] = &[0, 159, 146, 150, 255, 0, 1, 2];

    struct Fixture {
        repo: tempfile::TempDir,
    }

    impl Fixture {
        fn path(&self) -> &Path {
            self.repo.path()
        }

        fn read(&self, name: &str) -> Option<Vec<u8>> {
            std::fs::read(self.path().join(name)).ok()
        }

        fn status(&self) -> String {
            run_git(self.path(), &["status", "--porcelain", "--untracked-files=all"])
        }

        fn state(&self) -> (String, String) {
            (self.status(), run_git(self.path(), &["ls-files", "-s"]))
        }
    }

    fn fixture() -> Fixture {
        let repo = init_repo();
        let dir = repo.path();
        write(dir, "keep.txt", "keep\n");
        write(dir, "edit.txt", "one\ntwo\nthree\n");
        write(dir, "remove.txt", "remove me\n");
        write(dir, "run.sh", "#!/bin/sh\necho hi\n");
        std::fs::set_permissions(dir.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("image.bin"), BINARY).unwrap();
        symlink("keep.txt", dir.join("link")).unwrap();
        write(dir, ".gitignore", "ignored.log\n");
        commit(dir, "base");
        Fixture { repo }
    }

    fn dirty_before_the_run(fx: &Fixture) {
        let dir = fx.path();
        write(dir, "edit.txt", "one\ntwo changed by me\nthree\n");
        write(dir, "staged.txt", "staged\n");
        run_git(dir, &["add", "staged.txt"]);
        write(dir, "mine.txt", "my own untracked file\n");
    }

    fn the_run_edits_everything(fx: &Fixture) {
        let dir = fx.path();
        write(dir, "edit.txt", "ONE\ntwo changed by me\nthree\nfour\n");
        std::fs::remove_file(dir.join("remove.txt")).unwrap();
        std::fs::write(dir.join("image.bin"), [9u8, 8, 7, 0, 255]).unwrap();
        write(dir, "created.txt", "new\n");
        std::fs::create_dir_all(dir.join("deep/er")).unwrap();
        write(dir, "deep/er/file.txt", "nested\n");
        std::fs::set_permissions(dir.join("run.sh"), std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::remove_file(dir.join("link")).unwrap();
        symlink("edit.txt", dir.join("link")).unwrap();
        write(dir, "staged.txt", "staged then edited\n");
        run_git(dir, &["add", "created.txt"]);
        write(dir, "ignored.log", "noise\n");
    }

    fn id() -> RunId {
        RunId(7)
    }

    #[test]
    fn undo_restores_every_kind_of_change_exactly() {
        let fx = fixture();
        dirty_before_the_run(&fx);
        let state_before = fx.state();
        let mut snapshot = take(fx.path(), id()).unwrap();

        the_run_edits_everything(&fx);
        finish(fx.path(), &mut snapshot).unwrap();
        assert_ne!(fx.state(), state_before);

        let report = undo(fx.path(), &snapshot, false).unwrap();
        assert!(report.is_clean(), "{report:?}");

        assert_eq!(fx.state(), state_before);
        assert_eq!(fx.read("edit.txt").unwrap(), b"one\ntwo changed by me\nthree\n");
        assert_eq!(fx.read("remove.txt").unwrap(), b"remove me\n");
        assert_eq!(fx.read("image.bin").unwrap(), BINARY);
        assert_eq!(fx.read("staged.txt").unwrap(), b"staged\n");
        assert_eq!(fx.read("mine.txt").unwrap(), b"my own untracked file\n");
        assert!(!fx.path().join("created.txt").exists());
        assert!(!fx.path().join("deep").exists());
        assert_eq!(
            std::fs::metadata(fx.path().join("run.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert_eq!(
            std::fs::read_link(fx.path().join("link")).unwrap(),
            Path::new("keep.txt")
        );
        assert_eq!(fx.read("ignored.log").unwrap(), b"noise\n");
    }

    #[test]
    fn undoing_twice_changes_nothing_more() {
        let fx = fixture();
        let mut snapshot = take(fx.path(), id()).unwrap();
        the_run_edits_everything(&fx);
        finish(fx.path(), &mut snapshot).unwrap();
        undo(fx.path(), &snapshot, false).unwrap();
        let once = fx.state();
        let second = undo(fx.path(), &snapshot, false).unwrap();
        assert_eq!(second, UndoReport::default());
        assert_eq!(fx.state(), once);
    }

    #[test]
    fn show_change_lists_and_diffs_what_the_run_did() {
        let fx = fixture();
        dirty_before_the_run(&fx);
        let mut snapshot = take(fx.path(), id()).unwrap();
        the_run_edits_everything(&fx);
        finish(fx.path(), &mut snapshot).unwrap();

        let files = changed_files(fx.path(), &snapshot, Until::RunEnd).unwrap();
        let find = |path: &str| {
            files
                .iter()
                .find(|file| file.path == path)
                .unwrap_or_else(|| panic!("{path} {files:?}"))
        };
        assert_eq!(find("created.txt").status, FileStatus::Added);
        assert_eq!(find("deep/er/file.txt").status, FileStatus::Added);
        assert_eq!(find("remove.txt").status, FileStatus::Deleted);
        assert_eq!(find("edit.txt").status, FileStatus::Modified);
        assert_eq!((find("edit.txt").additions, find("edit.txt").deletions), (2, 1));
        assert!(
            files
                .iter()
                .all(|file| file.path != "mine.txt" && file.path != "keep.txt")
        );

        let one = diff(fx.path(), &snapshot, Until::RunEnd, &["edit.txt".to_string()]).unwrap();
        assert_eq!(one.len(), 1);
        let added: Vec<&str> = one[0]
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.kind == crate::diff::LineKind::Added)
            .map(|line| line.text.as_str())
            .collect();
        assert_eq!(added, ["ONE", "four"]);
        let binary = diff(fx.path(), &snapshot, Until::RunEnd, &["image.bin".to_string()]).unwrap();
        assert!(binary[0].is_binary);
        let all = diff(fx.path(), &snapshot, Until::RunEnd, &[]).unwrap();
        assert!(all.iter().any(|file| file.path == "created.txt"));
    }

    #[test]
    fn a_file_the_reviewer_edited_after_the_run_is_a_conflict() {
        let fx = fixture();
        let mut snapshot = take(fx.path(), id()).unwrap();
        write(fx.path(), "edit.txt", "run version\n");
        write(fx.path(), "created.txt", "new\n");
        finish(fx.path(), &mut snapshot).unwrap();
        write(fx.path(), "edit.txt", "my later edit\n");

        let report = undo(fx.path(), &snapshot, false).unwrap();
        assert_eq!(report.conflicts, ["edit.txt"]);
        assert_eq!(report.removed, ["created.txt"]);
        assert_eq!(fx.read("edit.txt").unwrap(), b"my later edit\n");

        let forced = undo(fx.path(), &snapshot, true).unwrap();
        assert_eq!(forced.restored, ["edit.txt"]);
        assert_eq!(fx.read("edit.txt").unwrap(), b"one\ntwo\nthree\n");
    }

    #[test]
    fn the_snapshot_touches_neither_the_index_nor_the_working_tree() {
        let fx = fixture();
        dirty_before_the_run(&fx);
        let state = fx.state();
        let index_file = fx.path().join(".git/index");
        let index_before = std::fs::read(&index_file).unwrap();
        let mut snapshot = take(fx.path(), id()).unwrap();
        finish(fx.path(), &mut snapshot).unwrap();
        assert_eq!(std::fs::read(&index_file).unwrap(), index_before);
        assert_eq!(fx.state(), state);
        assert!(changed_files(fx.path(), &snapshot, Until::RunEnd).unwrap().is_empty());
    }

    #[test]
    fn nothing_visible_is_added_to_git() {
        let fx = fixture();
        let mut snapshot = take(fx.path(), id()).unwrap();
        finish(fx.path(), &mut snapshot).unwrap();
        assert_eq!(run_git(fx.path(), &["stash", "list"]), "");
        assert_eq!(run_git(fx.path(), &["branch", "--list"]).trim(), "* main");
        assert_eq!(run_git(fx.path(), &["tag", "--list"]), "");
        assert_eq!(run_git(fx.path(), &["log", "--oneline"]).lines().count(), 1);
        let refs = run_git(fx.path(), &["for-each-ref", "--format=%(refname)"]);
        let ours: Vec<&str> = refs
            .lines()
            .filter(|name| name.starts_with("refs/diffpilot/"))
            .collect();
        assert_eq!(ours, ["refs/diffpilot/runs/7/after", "refs/diffpilot/runs/7/before"]);
        let tracked = run_git(fx.path(), &["ls-files"]);
        assert!(!tracked.contains("diffpilot"));

        forget(fx.path(), id()).unwrap();
        assert_eq!(run_git(fx.path(), &["for-each-ref", "refs/diffpilot"]), "");
    }

    #[test]
    fn files_are_hashed_without_line_ending_or_lfs_filters() {
        let fx = fixture();
        write(fx.path(), ".gitattributes", "*.txt text eol=lf\n");
        run_git(fx.path(), &["config", "core.autocrlf", "input"]);
        std::fs::write(fx.path().join("crlf.txt"), b"a\r\nb\r\n").unwrap();
        let mut snapshot = take(fx.path(), id()).unwrap();
        std::fs::remove_file(fx.path().join("crlf.txt")).unwrap();
        finish(fx.path(), &mut snapshot).unwrap();
        undo(fx.path(), &snapshot, false).unwrap();
        assert_eq!(fx.read("crlf.txt").unwrap(), b"a\r\nb\r\n");
    }

    #[test]
    fn works_in_a_repo_without_commits_and_from_a_subfolder() {
        let repo = init_repo();
        write(repo.path(), "a.txt", "a\n");
        std::fs::create_dir(repo.path().join("sub")).unwrap();
        let sub = repo.path().join("sub");
        let mut snapshot = take(&sub, id()).unwrap();
        write(repo.path(), "b.txt", "b\n");
        finish(&sub, &mut snapshot).unwrap();
        let files = changed_files(&sub, &snapshot, Until::RunEnd).unwrap();
        assert_eq!(
            files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(),
            ["b.txt"]
        );
        undo(&sub, &snapshot, false).unwrap();
        assert!(!repo.path().join("b.txt").exists());
        assert!(repo.path().join("a.txt").exists());
    }

    #[test]
    fn changes_since_the_start_include_what_happens_after_the_run() {
        let fx = fixture();
        let mut snapshot = take(fx.path(), id()).unwrap();
        write(fx.path(), "created.txt", "x\n");
        finish(fx.path(), &mut snapshot).unwrap();
        write(fx.path(), "later.txt", "y\n");
        let at_end = changed_files(fx.path(), &snapshot, Until::RunEnd).unwrap();
        let now = changed_files(fx.path(), &snapshot, Until::Now).unwrap();
        assert_eq!(at_end.len(), 1);
        assert_eq!(now.len(), 2);
        let unfinished = take(fx.path(), RunId(8)).unwrap();
        assert!(matches!(
            changed_files(fx.path(), &unfinished, Until::RunEnd),
            Err(SnapshotError::Unfinished)
        ));
    }
}
