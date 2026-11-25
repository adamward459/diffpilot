// Adapted from diffity (MIT License, Copyright (c) 2026 Kamran Ahmed).
// The license text is in core/THIRD_PARTY_LICENSES.

use crate::comparison::Comparison;
use crate::diff::{self, FileDiff};
use crate::exec;
use crate::git::GitError;
use crate::threads::Side;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StageError {
    #[error("this comparison does not include the working tree")]
    NotWorkingTree,
    #[error("`{0}` is not a path inside the repository")]
    BadPath(String),
    #[error("`{0}` has no change to act on")]
    NoChange(String),
    #[error("no hunk of `{path}` starts with `{header}`")]
    HunkNotFound { path: String, header: String },
    #[error("the selection contains no changed line")]
    EmptySelection,
    #[error("`{0}` is a binary file, so only the whole file can be changed")]
    Binary(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("the file changed since it was shown: {0}")]
    Stale(String),
    #[error(transparent)]
    Git(#[from] GitError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Stage,
    Unstage,
    /// Throws away unstaged changes. A file with staged changes keeps them.
    Discard,
    /// Throws away every uncommitted change of one file, staged or not. Whole files only.
    Revert,
}

/// The diff a selection refers to: the index against the working tree, or HEAD against the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Area {
    Unstaged,
    Staged,
}

impl Action {
    pub fn area(self) -> Area {
        match self {
            Self::Stage | Self::Discard | Self::Revert => Area::Unstaged,
            Self::Unstage => Area::Staged,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub side: Side,
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    pub fn new(side: Side, start: u32, end: u32) -> Self {
        Self {
            side,
            start: start.min(end),
            end: start.max(end),
        }
    }

    fn contains(&self, line: u32) -> bool {
        (self.start..=self.end).contains(&line)
    }
}

/// Line numbers refer to the diff of the action's `Area`: old is the index (or HEAD), new is the working tree (or index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    File,
    Hunk(String),
    Lines(Vec<LineRange>),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StageState {
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub renamed_from: Option<String>,
}

pub fn status(repo: &Path) -> Result<BTreeMap<String, StageState>, StageError> {
    let output = exec::git_ok(repo, &["status", "--porcelain", "-z", "--untracked-files=all"])?;
    Ok(parse_status(&output))
}

pub fn parse_status(output: &str) -> BTreeMap<String, StageState> {
    let mut states = BTreeMap::new();
    let mut fields = output.split('\0').filter(|field| !field.is_empty());
    while let Some(field) = fields.next() {
        let (Some(code), Some(path)) = (field.get(..2), field.get(3..)) else {
            continue;
        };
        let mut codes = code.chars();
        let (x, y) = (codes.next().unwrap_or(' '), codes.next().unwrap_or(' '));
        let renamed_from = matches!(x, 'R' | 'C')
            .then(|| fields.next().map(String::from))
            .flatten();
        states.insert(
            path.to_string(),
            StageState {
                staged: x != ' ' && x != '?',
                unstaged: y != ' ',
                untracked: code == "??",
                renamed_from,
            },
        );
    }
    states
}

pub fn load_area(repo: &Path, area: Area) -> Result<Vec<FileDiff>, StageError> {
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-U3", "-M"];
    if area == Area::Staged {
        args.push("--cached");
    }
    Ok(diff::in_tree_order(diff::parse(&exec::git_ok(repo, &args)?)))
}

pub fn apply(
    repo: &Path,
    comparison: &Comparison,
    action: Action,
    path: &str,
    target: &Target,
) -> Result<(), StageError> {
    if !comparison.includes_uncommitted() {
        return Err(StageError::NotWorkingTree);
    }
    check_path(path)?;
    let state = state_of(repo, path)?;
    match target {
        Target::File => whole_file(repo, action, path, &state),
        Target::Hunk(_) | Target::Lines(_) => partial(repo, action, path, target, &state),
    }
}

fn check_path(path: &str) -> Result<(), StageError> {
    let escapes = Path::new(path).components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_)
        )
    });
    if path.is_empty() || escapes || path.contains('\0') {
        return Err(StageError::BadPath(path.to_string()));
    }
    Ok(())
}

fn state_of(repo: &Path, path: &str) -> Result<StageState, StageError> {
    status(repo)?
        .remove(path)
        .ok_or_else(|| StageError::NoChange(path.to_string()))
}

fn literal(path: &str) -> String {
    format!(":(literal){path}")
}

fn whole_file(repo: &Path, action: Action, path: &str, state: &StageState) -> Result<(), StageError> {
    let literal_path = literal(path);
    match action {
        Action::Stage => {
            if !state.unstaged {
                return Err(StageError::NoChange(path.to_string()));
            }
            exec::git_ok(repo, &["add", "-A", "--", &literal_path])?;
        }
        Action::Unstage => {
            if !state.staged {
                return Err(StageError::NoChange(path.to_string()));
            }
            let mut args = vec!["reset", "-q", "--"];
            let old = state.renamed_from.as_deref().map(literal);
            let literal_old = old.as_deref();
            args.push(&literal_path);
            args.extend(literal_old);
            exec::git_ok(repo, &args)?;
        }
        Action::Discard => {
            if !state.unstaged {
                return Err(StageError::NoChange(path.to_string()));
            }
            discard_unstaged(repo, path, state)?;
        }
        Action::Revert => revert_to_head(repo, path, state)?,
    }
    Ok(())
}

fn discard_unstaged(repo: &Path, path: &str, state: &StageState) -> Result<(), StageError> {
    let literal_path = literal(path);
    if state.untracked {
        return remove_file(repo, path);
    }
    let intent_to_add = exec::git_ok(repo, &["diff", "--name-status", "--", &literal_path])?
        .trim_start()
        .starts_with('A');
    if intent_to_add {
        exec::git_ok(repo, &["rm", "-f", "-q", "--cached", "--", &literal_path])?;
        return remove_file(repo, path);
    }
    exec::git_ok(repo, &["checkout", "--", &literal_path])?;
    Ok(())
}

fn revert_to_head(repo: &Path, path: &str, state: &StageState) -> Result<(), StageError> {
    let literal_path = literal(path);
    if state.untracked {
        return remove_file(repo, path);
    }
    let restore = state.renamed_from.as_deref().unwrap_or(path);
    let in_head = exec::git_succeeds(repo, &["cat-file", "-e", &format!("HEAD:{restore}")]);
    if let Some(old) = &state.renamed_from {
        exec::git_ok(repo, &["rm", "-f", "-q", "--cached", "--", &literal_path])?;
        remove_file(repo, path)?;
        exec::git_ok(repo, &["checkout", "HEAD", "--", &literal(old)])?;
    } else if in_head {
        exec::git_ok(repo, &["checkout", "HEAD", "--", &literal_path])?;
    } else {
        exec::git_ok(repo, &["rm", "-f", "-q", "--cached", "--", &literal_path])?;
        remove_file(repo, path)?;
    }
    Ok(())
}

fn remove_file(repo: &Path, path: &str) -> Result<(), StageError> {
    let full = repo.join(path);
    if full.is_file() || full.is_symlink() {
        std::fs::remove_file(&full).map_err(|error| GitError::Failed(error.to_string()))?;
    }
    Ok(())
}

fn partial(repo: &Path, action: Action, path: &str, target: &Target, state: &StageState) -> Result<(), StageError> {
    if action == Action::Revert {
        return Err(StageError::Unsupported(
            "revert acts on whole files; use discard for hunks and lines".into(),
        ));
    }
    let area = action.area();
    let needed = match area {
        Area::Unstaged => state.unstaged,
        Area::Staged => state.staged,
    };
    if !needed {
        return Err(StageError::NoChange(path.to_string()));
    }
    if state.untracked {
        if action == Action::Discard {
            return Err(StageError::Unsupported(
                "an untracked file can only be discarded as a whole".into(),
            ));
        }
        exec::git_ok(repo, &["add", "-N", "--", &literal(path)])?;
    }
    let raw = read_file_diff(repo, area, path, state)?;
    if raw.binary {
        return Err(StageError::Binary(path.to_string()));
    }
    let select = |hunk: &RawHunk, line: &RawLine| match target {
        Target::File => true,
        Target::Hunk(header) => hunk.header.trim_end() == header.trim_end(),
        Target::Lines(ranges) => ranges.iter().any(|range| match (range.side, line.kind) {
            (Side::New, Kind::Added) => line.new.is_some_and(|number| range.contains(number)),
            (Side::Old, Kind::Deleted) => line.old.is_some_and(|number| range.contains(number)),
            _ => false,
        }),
    };
    if let Target::Hunk(header) = target
        && !raw.hunks.iter().any(|hunk| hunk.header.trim_end() == header.trim_end())
    {
        return Err(StageError::HunkNotFound {
            path: path.to_string(),
            header: header.clone(),
        });
    }
    let covers_everything = raw.hunks.iter().all(|hunk| {
        hunk.lines
            .iter()
            .all(|line| line.kind == Kind::Context || select(hunk, line))
    });
    if covers_everything && raw.hunks.iter().any(|hunk| hunk.has_changes()) {
        return whole_file(repo, action, path, state);
    }
    let sense = if action == Action::Stage {
        Sense::Forward
    } else {
        Sense::Reverse
    };
    let patch = build_patch(path, &raw.hunks, &select, sense).ok_or(StageError::EmptySelection)?;
    let mut args = vec!["apply", "--whitespace=nowarn"];
    if sense == Sense::Reverse {
        args.push("-R");
    }
    if action != Action::Discard {
        args.push("--cached");
    }
    args.push("-");
    let output = exec::git(repo, &args, Some(patch.as_bytes()))?;
    if output.ok {
        Ok(())
    } else {
        Err(StageError::Stale(output.text()))
    }
}

fn read_file_diff(repo: &Path, area: Area, path: &str, state: &StageState) -> Result<RawFile, StageError> {
    let literal_path = literal(path);
    let literal_old = state.renamed_from.as_deref().map(literal);
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-U3", "-M"];
    if area == Area::Staged {
        args.push("--cached");
    }
    args.extend(["--", &literal_path]);
    args.extend(literal_old.as_deref().filter(|_| area == Area::Staged));
    Ok(parse_raw(&exec::git_ok(repo, &args)?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Context,
    Added,
    Deleted,
}

#[derive(Debug, Clone)]
struct RawLine {
    kind: Kind,
    text: String,
    old: Option<u32>,
    new: Option<u32>,
    no_newline: bool,
}

#[derive(Debug, Clone)]
struct RawHunk {
    header: String,
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
    section: String,
    lines: Vec<RawLine>,
}

impl RawHunk {
    fn has_changes(&self) -> bool {
        self.lines.iter().any(|line| line.kind != Kind::Context)
    }

    fn old_position(&self) -> i64 {
        i64::from(self.old_start) + i64::from(self.old_count == 0)
    }

    fn new_position(&self) -> i64 {
        i64::from(self.new_start) + i64::from(self.new_count == 0)
    }
}

#[derive(Debug, Default)]
struct RawFile {
    binary: bool,
    hunks: Vec<RawHunk>,
}

fn parse_raw(text: &str) -> RawFile {
    let mut file = RawFile::default();
    let mut current: Option<RawHunk> = None;
    let (mut old, mut new) = (0, 0);
    let finish = |hunk: &mut Option<RawHunk>, file: &mut RawFile| {
        file.hunks.extend(hunk.take());
    };
    for line in text.split('\n') {
        if line.starts_with("diff --git ") {
            finish(&mut current, &mut file);
        } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            file.binary = true;
        } else if line.starts_with("@@") {
            finish(&mut current, &mut file);
            if let Some((old_start, old_count, new_start, new_count, section)) = parse_header(line) {
                (old, new) = (old_start, new_start);
                current = Some(RawHunk {
                    header: line.to_string(),
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                    section,
                    lines: Vec::new(),
                });
            }
        } else if let Some(hunk) = current.as_mut() {
            let Some(first) = line.chars().next() else { continue };
            let text = line[first.len_utf8()..].to_string();
            let kind = match first {
                '+' => Kind::Added,
                '-' => Kind::Deleted,
                ' ' => Kind::Context,
                '\\' => {
                    if let Some(last) = hunk.lines.last_mut() {
                        last.no_newline = true;
                    }
                    continue;
                }
                _ => continue,
            };
            let numbers = (
                (kind != Kind::Added).then_some(old),
                (kind != Kind::Deleted).then_some(new),
            );
            if kind != Kind::Added {
                old += 1;
            }
            if kind != Kind::Deleted {
                new += 1;
            }
            hunk.lines.push(RawLine {
                kind,
                text,
                old: numbers.0,
                new: numbers.1,
                no_newline: false,
            });
        }
    }
    finish(&mut current, &mut file);
    file
}

fn parse_header(line: &str) -> Option<(u32, u32, u32, u32, String)> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, section) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let range = |text: &str| -> Option<(u32, u32)> {
        match text.split_once(',') {
            Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
            None => Some((text.parse().ok()?, 1)),
        }
    };
    let (old_start, old_count) = range(old)?;
    let (new_start, new_count) = range(new)?;
    Some((old_start, old_count, new_start, new_count, section.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sense {
    /// Applied to the old side: unselected additions vanish, unselected deletions stay as context.
    Forward,
    /// Applied in reverse to the new side: unselected deletions vanish, unselected additions stay as context.
    Reverse,
}

fn build_patch(
    path: &str,
    hunks: &[RawHunk],
    select: &dyn Fn(&RawHunk, &RawLine) -> bool,
    sense: Sense,
) -> Option<String> {
    let mut body = String::new();
    let mut delta: i64 = 0;
    for hunk in hunks {
        let mut lines: Vec<(char, &RawLine)> = Vec::new();
        let mut any = false;
        for line in &hunk.lines {
            let selected = line.kind != Kind::Context && select(hunk, line);
            any |= selected;
            let prefix = match (line.kind, selected, sense) {
                (Kind::Context, _, _) => ' ',
                (Kind::Added, true, _) => '+',
                (Kind::Deleted, true, _) => '-',
                (Kind::Added, false, Sense::Forward) | (Kind::Deleted, false, Sense::Reverse) => continue,
                (Kind::Added, false, Sense::Reverse) | (Kind::Deleted, false, Sense::Forward) => ' ',
            };
            lines.push((prefix, line));
        }
        if !any {
            continue;
        }
        let old_count = lines.iter().filter(|(prefix, _)| *prefix != '+').count() as i64;
        let new_count = lines.iter().filter(|(prefix, _)| *prefix != '-').count() as i64;
        let start = |position: i64, count: i64| if count > 0 { position } else { position - 1 };
        let (old_start, new_start) = match sense {
            Sense::Forward => (i64::from(hunk.old_start), start(hunk.old_position() + delta, new_count)),
            Sense::Reverse => (start(hunk.new_position() - delta, old_count), i64::from(hunk.new_start)),
        };
        body.push_str(&format!(
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@{}\n",
            hunk.section
        ));
        for (prefix, line) in lines {
            body.push(prefix);
            body.push_str(&line.text);
            body.push('\n');
            if line.no_newline {
                body.push_str("\\ No newline at end of file\n");
            }
        }
        delta += new_count - old_count;
    }
    if body.is_empty() {
        return None;
    }
    let name = patch_name(path);
    let tab = if path.contains(' ') { "\t" } else { "" };
    Some(format!(
        "diff --git a/{name} b/{name}\n--- a/{name}{tab}\n+++ b/{name}{tab}\n{body}"
    ))
}

fn patch_name(path: &str) -> String {
    if path.contains(['"', '\\', '\n', '\t']) {
        let escaped = path
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t");
        return format!("\"{escaped}\"");
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{commit, git, init_repo, write};
    use tempfile::TempDir;

    const NUMBERS: &str = "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19\nl20\n";

    fn working() -> Comparison {
        Comparison::WorkingTree
    }

    fn repo_with(files: &[(&str, &str)]) -> TempDir {
        let dir = init_repo();
        for (name, text) in files {
            if let Some(parent) = Path::new(name).parent() {
                std::fs::create_dir_all(dir.path().join(parent)).unwrap();
            }
            write(dir.path(), name, text);
        }
        commit(dir.path(), "base");
        dir
    }

    fn run(repo: &Path, action: Action, path: &str, target: Target) -> Result<(), StageError> {
        apply(repo, &working(), action, path, &target)
    }

    fn staged_text(repo: &Path, path: &str) -> String {
        git(repo, &["show", &format!(":{path}")])
    }

    fn read(repo: &Path, path: &str) -> String {
        std::fs::read_to_string(repo.join(path)).unwrap()
    }

    fn hunk_headers(repo: &Path, area: Area, path: &str) -> Vec<String> {
        load_area(repo, area)
            .unwrap()
            .into_iter()
            .filter(|file| file.path == path)
            .flat_map(|file| file.hunks.into_iter().map(|hunk| hunk.header))
            .collect()
    }

    fn two_hunk_edit(repo: &Path) -> String {
        let edited = NUMBERS.replace("l2\n", "L2\n").replace("l18\n", "L18\n");
        write(repo, "n.txt", &edited);
        edited
    }

    #[test]
    fn refuses_comparisons_without_the_working_tree() {
        let dir = repo_with(&[("a.txt", "x\n")]);
        write(dir.path(), "a.txt", "y\n");
        for comparison in [
            Comparison::checked_out("main", false),
            Comparison::branch("main", "main"),
        ] {
            assert_eq!(
                apply(dir.path(), &comparison, Action::Stage, "a.txt", &Target::File),
                Err(StageError::NotWorkingTree)
            );
        }
        assert_eq!(
            apply(
                dir.path(),
                &Comparison::checked_out("main", true),
                Action::Stage,
                "a.txt",
                &Target::File
            ),
            Ok(())
        );
    }

    #[test]
    fn refuses_paths_outside_the_repository() {
        let dir = repo_with(&[("a.txt", "x\n")]);
        for path in ["", "../a.txt", "/etc/passwd"] {
            assert_eq!(
                run(dir.path(), Action::Stage, path, Target::File),
                Err(StageError::BadPath(path.to_string()))
            );
        }
    }

    #[test]
    fn reports_a_file_without_changes() {
        let dir = repo_with(&[("a.txt", "x\n")]);
        assert_eq!(
            run(dir.path(), Action::Stage, "a.txt", Target::File),
            Err(StageError::NoChange("a.txt".into()))
        );
    }

    #[test]
    fn stages_and_unstages_a_modified_file() {
        let dir = repo_with(&[("a.txt", "x\n")]);
        let repo = dir.path();
        write(repo, "a.txt", "y\n");
        run(repo, Action::Stage, "a.txt", Target::File).unwrap();
        assert_eq!(staged_text(repo, "a.txt"), "y\n");
        assert_eq!(
            run(repo, Action::Stage, "a.txt", Target::File),
            Err(StageError::NoChange("a.txt".into()))
        );
        run(repo, Action::Unstage, "a.txt", Target::File).unwrap();
        assert_eq!(staged_text(repo, "a.txt"), "x\n");
        assert_eq!(read(repo, "a.txt"), "y\n");
    }

    #[test]
    fn stages_new_and_deleted_files() {
        let dir = repo_with(&[("gone.txt", "bye\n")]);
        let repo = dir.path();
        write(repo, "new.txt", "hello\n");
        std::fs::remove_file(repo.join("gone.txt")).unwrap();
        run(repo, Action::Stage, "new.txt", Target::File).unwrap();
        run(repo, Action::Stage, "gone.txt", Target::File).unwrap();
        let staged = status(repo).unwrap();
        assert!(staged["new.txt"].staged && !staged["new.txt"].unstaged);
        assert!(staged["gone.txt"].staged);
        run(repo, Action::Unstage, "gone.txt", Target::File).unwrap();
        run(repo, Action::Unstage, "new.txt", Target::File).unwrap();
        let after = status(repo).unwrap();
        assert!(after["new.txt"].untracked);
        assert!(after["gone.txt"].unstaged && !after["gone.txt"].staged);
    }

    #[test]
    fn discards_a_modified_file_but_keeps_what_is_staged() {
        let dir = repo_with(&[("a.txt", "x\n")]);
        let repo = dir.path();
        write(repo, "a.txt", "staged\n");
        git(repo, &["add", "a.txt"]);
        write(repo, "a.txt", "staged\nmore\n");
        run(repo, Action::Discard, "a.txt", Target::File).unwrap();
        assert_eq!(read(repo, "a.txt"), "staged\n");
        assert_eq!(staged_text(repo, "a.txt"), "staged\n");
    }

    #[test]
    fn discards_untracked_intent_to_add_and_deleted_files() {
        let dir = repo_with(&[("gone.txt", "bye\n")]);
        let repo = dir.path();
        write(repo, "untracked.txt", "u\n");
        write(repo, "ita.txt", "i\n");
        git(repo, &["add", "-N", "ita.txt"]);
        std::fs::remove_file(repo.join("gone.txt")).unwrap();
        for path in ["untracked.txt", "ita.txt", "gone.txt"] {
            run(repo, Action::Discard, path, Target::File).unwrap();
        }
        assert!(!repo.join("untracked.txt").exists());
        assert!(!repo.join("ita.txt").exists());
        assert_eq!(read(repo, "gone.txt"), "bye\n");
        assert!(status(repo).unwrap().is_empty());
    }

    #[test]
    fn reverts_staged_and_unstaged_changes_to_head() {
        let dir = repo_with(&[("a.txt", "x\n")]);
        let repo = dir.path();
        write(repo, "a.txt", "one\n");
        git(repo, &["add", "a.txt"]);
        write(repo, "a.txt", "two\n");
        write(repo, "new.txt", "n\n");
        git(repo, &["add", "new.txt"]);
        run(repo, Action::Revert, "a.txt", Target::File).unwrap();
        run(repo, Action::Revert, "new.txt", Target::File).unwrap();
        assert_eq!(read(repo, "a.txt"), "x\n");
        assert!(!repo.join("new.txt").exists());
        assert!(status(repo).unwrap().is_empty());
    }

    #[test]
    fn unstages_and_reverts_a_renamed_file() {
        let dir = repo_with(&[("old.txt", "content\n")]);
        let repo = dir.path();
        git(repo, &["mv", "old.txt", "new.txt"]);
        let states = status(repo).unwrap();
        assert_eq!(states["new.txt"].renamed_from.as_deref(), Some("old.txt"));
        run(repo, Action::Unstage, "new.txt", Target::File).unwrap();
        let after = status(repo).unwrap();
        assert!(after["new.txt"].untracked && after["old.txt"].unstaged);

        git(repo, &["add", "-A"]);
        run(repo, Action::Revert, "new.txt", Target::File).unwrap();
        assert_eq!(read(repo, "old.txt"), "content\n");
        assert!(!repo.join("new.txt").exists());
        assert!(status(repo).unwrap().is_empty());
    }

    #[test]
    fn stages_unstages_and_discards_one_hunk() {
        let dir = repo_with(&[("n.txt", NUMBERS)]);
        let repo = dir.path();
        let edited = two_hunk_edit(repo);
        let headers = hunk_headers(repo, Area::Unstaged, "n.txt");
        assert_eq!(headers.len(), 2);

        run(repo, Action::Stage, "n.txt", Target::Hunk(headers[0].clone())).unwrap();
        assert_eq!(staged_text(repo, "n.txt"), NUMBERS.replace("l2\n", "L2\n"));
        assert_eq!(hunk_headers(repo, Area::Unstaged, "n.txt").len(), 1);

        let staged_headers = hunk_headers(repo, Area::Staged, "n.txt");
        run(repo, Action::Unstage, "n.txt", Target::Hunk(staged_headers[0].clone())).unwrap();
        assert_eq!(staged_text(repo, "n.txt"), NUMBERS);

        let headers = hunk_headers(repo, Area::Unstaged, "n.txt");
        run(repo, Action::Discard, "n.txt", Target::Hunk(headers[1].clone())).unwrap();
        assert_eq!(read(repo, "n.txt"), edited.replace("L18\n", "l18\n"));
    }

    #[test]
    fn staging_a_hunk_that_is_gone_reports_which_one() {
        let dir = repo_with(&[("n.txt", NUMBERS)]);
        two_hunk_edit(dir.path());
        assert_eq!(
            run(
                dir.path(),
                Action::Stage,
                "n.txt",
                Target::Hunk("@@ -1,1 +1,1 @@".into())
            ),
            Err(StageError::HunkNotFound {
                path: "n.txt".into(),
                header: "@@ -1,1 +1,1 @@".into()
            })
        );
    }

    #[test]
    fn stages_only_the_selected_lines_of_a_hunk() {
        let dir = repo_with(&[("n.txt", NUMBERS)]);
        let repo = dir.path();
        let edited = NUMBERS
            .replace("l5\n", "")
            .replace("l6\n", "a\nb\nc\n")
            .replace("l7\n", "");
        write(repo, "n.txt", &edited);
        let lines = vec![LineRange::new(Side::New, 6, 7), LineRange::new(Side::Old, 5, 5)];
        run(repo, Action::Stage, "n.txt", Target::Lines(lines)).unwrap();
        let expected = NUMBERS.replace("l5\n", "").replace("l7\n", "l7\nb\nc\n");
        assert_eq!(staged_text(repo, "n.txt"), expected);
        assert_eq!(read(repo, "n.txt"), edited);

        let rest = hunk_headers(repo, Area::Unstaged, "n.txt");
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn unstages_and_discards_only_the_selected_lines() {
        let dir = repo_with(&[("n.txt", NUMBERS)]);
        let repo = dir.path();
        let edited = NUMBERS.replace("l5\n", "x1\nx2\nx3\n");
        write(repo, "n.txt", &edited);
        git(repo, &["add", "n.txt"]);

        run(
            repo,
            Action::Unstage,
            "n.txt",
            Target::Lines(vec![LineRange::new(Side::New, 6, 6)]),
        )
        .unwrap();
        assert_eq!(staged_text(repo, "n.txt"), NUMBERS.replace("l5\n", "x1\nx3\n"));

        let unstaged = hunk_headers(repo, Area::Unstaged, "n.txt");
        assert_eq!(unstaged.len(), 1);
        run(
            repo,
            Action::Discard,
            "n.txt",
            Target::Lines(vec![LineRange::new(Side::New, 6, 6)]),
        )
        .unwrap();
        assert_eq!(read(repo, "n.txt"), NUMBERS.replace("l5\n", "x1\nx3\n"));
        assert!(hunk_headers(repo, Area::Unstaged, "n.txt").is_empty());
    }

    #[test]
    fn stages_lines_of_a_new_file_through_intent_to_add() {
        let dir = repo_with(&[("a.txt", "x\n")]);
        let repo = dir.path();
        write(repo, "new.txt", "one\ntwo\nthree\nfour\n");
        run(
            repo,
            Action::Stage,
            "new.txt",
            Target::Lines(vec![LineRange::new(Side::New, 2, 3)]),
        )
        .unwrap();
        assert_eq!(staged_text(repo, "new.txt"), "two\nthree\n");
        assert_eq!(read(repo, "new.txt"), "one\ntwo\nthree\nfour\n");

        run(
            repo,
            Action::Unstage,
            "new.txt",
            Target::Lines(vec![LineRange::new(Side::New, 1, 1)]),
        )
        .unwrap();
        assert_eq!(staged_text(repo, "new.txt"), "three\n");
    }

    #[test]
    fn selecting_every_line_of_a_new_or_deleted_file_acts_on_the_file() {
        let dir = repo_with(&[("gone.txt", "a\nb\n")]);
        let repo = dir.path();
        write(repo, "new.txt", "one\ntwo\n");
        git(repo, &["add", "-N", "new.txt"]);
        std::fs::remove_file(repo.join("gone.txt")).unwrap();

        run(
            repo,
            Action::Stage,
            "new.txt",
            Target::Lines(vec![LineRange::new(Side::New, 1, 2)]),
        )
        .unwrap();
        run(
            repo,
            Action::Stage,
            "gone.txt",
            Target::Lines(vec![LineRange::new(Side::Old, 1, 2)]),
        )
        .unwrap();
        let states = status(repo).unwrap();
        assert!(states["new.txt"].staged && !states["new.txt"].unstaged);
        assert!(states["gone.txt"].staged && !states["gone.txt"].unstaged);
        let removed = git(repo, &["diff", "--cached", "--name-status"]);
        assert!(removed.contains("D\tgone.txt"), "{removed}");
    }

    #[test]
    fn stages_part_of_a_deleted_file() {
        let dir = repo_with(&[("gone.txt", "a\nb\nc\n")]);
        let repo = dir.path();
        std::fs::remove_file(repo.join("gone.txt")).unwrap();
        run(
            repo,
            Action::Stage,
            "gone.txt",
            Target::Lines(vec![LineRange::new(Side::Old, 1, 1)]),
        )
        .unwrap();
        assert_eq!(staged_text(repo, "gone.txt"), "b\nc\n");
        assert!(!repo.join("gone.txt").exists());
    }

    #[test]
    fn discards_a_hunk_of_an_intent_to_add_file() {
        let dir = repo_with(&[("a.txt", "x\n")]);
        let repo = dir.path();
        write(repo, "new.txt", "one\ntwo\nthree\n");
        git(repo, &["add", "-N", "new.txt"]);
        run(
            repo,
            Action::Discard,
            "new.txt",
            Target::Lines(vec![LineRange::new(Side::New, 2, 2)]),
        )
        .unwrap();
        assert_eq!(read(repo, "new.txt"), "one\nthree\n");
    }

    #[test]
    fn an_untracked_file_cannot_be_discarded_by_line() {
        let dir = repo_with(&[("a.txt", "x\n")]);
        write(dir.path(), "new.txt", "one\n");
        assert!(matches!(
            run(
                dir.path(),
                Action::Discard,
                "new.txt",
                Target::Lines(vec![LineRange::new(Side::New, 1, 1)])
            ),
            Err(StageError::Unsupported(_))
        ));
    }

    #[test]
    fn an_empty_selection_is_an_error() {
        let dir = repo_with(&[("n.txt", NUMBERS)]);
        two_hunk_edit(dir.path());
        assert_eq!(
            run(
                dir.path(),
                Action::Stage,
                "n.txt",
                Target::Lines(vec![LineRange::new(Side::New, 10, 12)])
            ),
            Err(StageError::EmptySelection)
        );
    }

    #[test]
    fn keeps_a_missing_newline_at_the_end_of_the_file() {
        let dir = repo_with(&[("a.txt", "one\ntwo\nthree")]);
        let repo = dir.path();
        write(repo, "a.txt", "ONE\ntwo\nTHREE");
        run(
            repo,
            Action::Stage,
            "a.txt",
            Target::Lines(vec![LineRange::new(Side::Old, 1, 1), LineRange::new(Side::New, 1, 1)]),
        )
        .unwrap();
        assert_eq!(staged_text(repo, "a.txt"), "ONE\ntwo\nthree");
    }

    #[test]
    fn handles_paths_with_spaces_and_unicode() {
        let dir = repo_with(&[("my dir/é file.txt", NUMBERS)]);
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("my dir")).ok();
        write(repo, "my dir/é file.txt", &NUMBERS.replace("l3\n", "L3\n"));
        let headers = hunk_headers(repo, Area::Unstaged, "my dir/é file.txt");
        run(
            repo,
            Action::Stage,
            "my dir/é file.txt",
            Target::Hunk(headers[0].clone()),
        )
        .unwrap();
        assert!(staged_text(repo, "my dir/é file.txt").contains("L3"));
    }

    #[test]
    fn binary_files_only_support_whole_file_actions() {
        let dir = repo_with(&[("a.txt", "x\n")]);
        let repo = dir.path();
        std::fs::write(repo.join("b.bin"), [0u8, 1, 2]).unwrap();
        git(repo, &["add", "b.bin"]);
        git(repo, &["commit", "-q", "-m", "bin"]);
        std::fs::write(repo.join("b.bin"), [0u8, 9, 2, 3]).unwrap();
        assert_eq!(
            run(
                repo,
                Action::Stage,
                "b.bin",
                Target::Lines(vec![LineRange::new(Side::New, 1, 1)])
            ),
            Err(StageError::Binary("b.bin".into()))
        );
        run(repo, Action::Stage, "b.bin", Target::File).unwrap();
    }

    #[test]
    fn parses_status_output() {
        let states = parse_status("M  a.txt\0 M b.txt\0?? c.txt\0R  new.txt\0old.txt\0AM d.txt\0");
        assert!(states["a.txt"].staged && !states["a.txt"].unstaged);
        assert!(!states["b.txt"].staged && states["b.txt"].unstaged);
        assert!(states["c.txt"].untracked && states["c.txt"].unstaged);
        assert_eq!(states["new.txt"].renamed_from.as_deref(), Some("old.txt"));
        assert!(states["d.txt"].staged && states["d.txt"].unstaged);
        assert_eq!(states.len(), 5);
    }
}
