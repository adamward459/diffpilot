use crate::changes::FileStatus;
use crate::comparison::Comparison;
use crate::git::{self, GitError};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

pub const DIFF_ARGS: [&str; 6] = ["-c", "core.quotepath=false", "diff", "-U3", "-M", "--no-color"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineKind {
    Context,
    Added,
    Deleted,
}

impl LineKind {
    fn code(self) -> &'static str {
        match self {
            Self::Context => "c",
            Self::Added => "a",
            Self::Deleted => "d",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub is_binary: bool,
    pub additions: u32,
    pub deletions: u32,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    pub fn new(path: &str, status: FileStatus) -> Self {
        Self {
            path: path.to_string(),
            old_path: None,
            status,
            is_binary: false,
            additions: 0,
            deletions: 0,
            hunks: Vec::new(),
        }
    }

    /// A type change (symlink to file) is two `diff --git` blocks for one path; the sidebar shows one.
    fn absorb(&mut self, other: FileDiff) {
        self.status = FileStatus::Modified;
        self.is_binary |= other.is_binary;
        self.additions += other.additions;
        self.deletions += other.deletions;
        self.hunks.extend(other.hunks);
    }

    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        let mut add = |text: &str| {
            hasher.update(text.as_bytes());
            hasher.update([0]);
        };
        add(self.status.code());
        add(self.old_path.as_deref().unwrap_or(""));
        add(if self.is_binary { "binary" } else { "text" });
        for hunk in &self.hunks {
            add(&hunk.header);
            for line in &hunk.lines {
                add(line.kind.code());
                add(&line.text);
            }
        }
        hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

pub fn load(repo: &Path, comparison: &Comparison) -> Result<Vec<FileDiff>, GitError> {
    load_revisions(repo, &comparison.revisions())
}

pub fn load_revisions(repo: &Path, revisions: &[String]) -> Result<Vec<FileDiff>, GitError> {
    let args: Vec<&str> = DIFF_ARGS
        .iter()
        .copied()
        .chain(revisions.iter().map(String::as_str))
        .collect();
    Ok(in_tree_order(parse(&git::run(repo, &args)?)))
}

/// Fingerprints of the files named in `paths`, for `ViewedFiles::clear_changed`.
pub fn fingerprints(files: &[FileDiff], paths: &std::collections::HashSet<String>) -> HashMap<String, String> {
    files
        .iter()
        .filter(|file| paths.contains(&file.path))
        .map(|file| (file.path.clone(), file.fingerprint()))
        .collect()
}

pub fn in_tree_order(files: Vec<FileDiff>) -> Vec<FileDiff> {
    crate::tree::in_tree_order(files, |file| &file.path)
}

pub fn parse(text: &str) -> Vec<FileDiff> {
    let mut parser = Parser::default();
    for line in text.as_bytes().split(|&byte| byte == b'\n') {
        parser.feed(line);
    }
    parser.finish()
}

#[derive(Default)]
struct Parser {
    files: Vec<FileDiff>,
    file: Option<FileDiff>,
    hunk: Option<Hunk>,
    old: u32,
    new: u32,
}

impl Parser {
    fn feed(&mut self, line: &[u8]) {
        if line.starts_with(b"diff --git ") {
            self.finish_file();
            self.file = Some(FileDiff::new(&header_path(line), FileStatus::Modified));
        } else if self.hunk.is_none() {
            self.feed_header(line);
        } else if line.starts_with(b"@@") {
            self.start_hunk(line);
        } else {
            self.feed_body(line);
        }
    }

    fn feed_header(&mut self, line: &[u8]) {
        let Some(file) = self.file.as_mut() else { return };
        if line.starts_with(b"new file") {
            file.status = FileStatus::Added;
        } else if line.starts_with(b"deleted file") {
            file.status = FileStatus::Deleted;
        } else if let Some(rest) = line.strip_prefix(b"rename from ") {
            file.status = FileStatus::Renamed;
            file.old_path = Some(path_field(rest));
        } else if let Some(rest) = line.strip_prefix(b"rename to ") {
            file.path = path_field(rest);
        } else if line.starts_with(b"Binary files ") || line.starts_with(b"GIT binary patch") {
            file.is_binary = true;
        } else if line.starts_with(b"@@") {
            self.start_hunk(line);
        }
    }

    fn start_hunk(&mut self, line: &[u8]) {
        self.finish_hunk();
        (self.old, self.new) = hunk_starts(line);
        self.hunk = Some(Hunk {
            header: string(line),
            lines: Vec::new(),
        });
    }

    fn feed_body(&mut self, line: &[u8]) {
        let Some((&first, rest)) = line.split_first() else {
            return;
        };
        let text = string(rest);
        let (Some(hunk), Some(file)) = (self.hunk.as_mut(), self.file.as_mut()) else {
            return;
        };
        match first {
            b'+' => {
                hunk.lines.push(DiffLine {
                    kind: LineKind::Added,
                    old: None,
                    new: Some(self.new),
                    text,
                });
                self.new += 1;
                file.additions += 1;
            }
            b'-' => {
                hunk.lines.push(DiffLine {
                    kind: LineKind::Deleted,
                    old: Some(self.old),
                    new: None,
                    text,
                });
                self.old += 1;
                file.deletions += 1;
            }
            b' ' => {
                hunk.lines.push(DiffLine {
                    kind: LineKind::Context,
                    old: Some(self.old),
                    new: Some(self.new),
                    text,
                });
                self.old += 1;
                self.new += 1;
            }
            _ => {}
        }
    }

    fn finish_hunk(&mut self) {
        if let (Some(hunk), Some(file)) = (self.hunk.take(), self.file.as_mut()) {
            file.hunks.push(hunk);
        }
    }

    fn finish_file(&mut self) {
        self.finish_hunk();
        let Some(file) = self.file.take() else { return };
        match self.files.last_mut() {
            Some(last) if last.path == file.path => last.absorb(file),
            _ => self.files.push(file),
        }
    }

    fn finish(mut self) -> Vec<FileDiff> {
        self.finish_file();
        self.files
    }
}

fn string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn header_path(line: &[u8]) -> String {
    let rest = line.get("diff --git ".len()..).unwrap_or(&[]);
    let new = if let Some((_, after)) = unquote(rest) {
        after.strip_prefix(b" ").unwrap_or(after)
    } else if rest.ends_with(b"\"") {
        let start = rest
            .windows(2)
            .position(|pair| pair == b" \"")
            .map_or(0, |index| index + 1);
        &rest[start..]
    } else {
        return unquoted_header_path(rest.get("a/".len()..).unwrap_or(&[]));
    };
    let new = unquote(new).map_or_else(|| new.to_vec(), |(path, _)| path);
    string(new.strip_prefix(b"b/").unwrap_or(&new))
}

// For `diff --git a/P b/P` the two halves have equal length, which keeps spaces in P unambiguous.
fn unquoted_header_path(rest: &[u8]) -> String {
    if rest.len() >= 5 && rest.len() % 2 == 1 {
        return string(&rest[..(rest.len() - 3) / 2]);
    }
    let separator = b" b/";
    (0..rest.len())
        .rev()
        .find(|&index| rest[index..].starts_with(separator))
        .map_or_else(|| string(rest), |index| string(&rest[index + separator.len()..]))
}

fn path_field(text: &[u8]) -> String {
    match unquote(text) {
        Some((path, _)) => string(&path),
        None => string(text),
    }
}

/// Git C-quotes a path with a control byte, `"`, `\\`, or non-ASCII (unless core.quotepath=false).
/// Returns the path bytes and the text after the closing quote.
fn unquote(text: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let mut rest = text.strip_prefix(b"\"")?;
    let mut path = Vec::new();
    loop {
        let (&byte, tail) = rest.split_first()?;
        rest = tail;
        match byte {
            b'"' => return Some((path, rest)),
            b'\\' => {
                let (&escape, tail) = rest.split_first()?;
                rest = tail;
                path.push(match escape {
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'v' => 0x0b,
                    b'f' => 0x0c,
                    b'r' => b'\r',
                    b'0'..=b'7' => {
                        let digits = rest.get(..2)?;
                        rest = &rest[2..];
                        let value = [escape, digits[0], digits[1]].iter().try_fold(0u32, |value, &digit| {
                            (b'0'..=b'7')
                                .contains(&digit)
                                .then(|| value * 8 + u32::from(digit - b'0'))
                        })?;
                        u8::try_from(value).ok()?
                    }
                    other => other,
                });
            }
            _ => path.push(byte),
        }
    }
}

fn hunk_starts(line: &[u8]) -> (u32, u32) {
    let mut numbers: Vec<u32> = Vec::new();
    let mut current: Option<u32> = None;
    let mut after_comma = false;
    for &byte in line.iter().skip(2) {
        if byte.is_ascii_digit() {
            if !after_comma {
                let digit = u32::from(byte - b'0');
                current = Some(current.unwrap_or(0).saturating_mul(10).saturating_add(digit));
            }
        } else if byte == b',' {
            numbers.extend(current.take());
            after_comma = true;
        } else {
            numbers.extend(current.take());
            after_comma = false;
            if byte == b'@' {
                break;
            }
        }
    }
    (
        numbers.first().copied().unwrap_or(0),
        numbers.get(1).copied().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_lines(lines: &[&str]) -> Vec<FileDiff> {
        parse(&(lines.join("\n") + "\n"))
    }

    fn line(kind: LineKind, old: Option<u32>, new: Option<u32>, text: &str) -> DiffLine {
        DiffLine {
            kind,
            old,
            new,
            text: text.into(),
        }
    }

    #[test]
    fn parses_hunks_with_line_numbers() {
        let files = parse_lines(&[
            "diff --git a/src/a.ts b/src/a.ts",
            "index 111..222 100644",
            "--- a/src/a.ts",
            "+++ b/src/a.ts",
            "@@ -3,4 +3,5 @@ func x()",
            " keep",
            "-gone",
            "+new one",
            "+new two",
            " tail",
            "@@ -20 +21 @@",
            "-single",
            "+changed",
        ]);
        assert_eq!(files.len(), 1);
        let file = &files[0];
        assert_eq!(file.path, "src/a.ts");
        assert_eq!(file.status, FileStatus::Modified);
        assert_eq!((file.additions, file.deletions), (3, 2));
        let headers: Vec<_> = file.hunks.iter().map(|h| h.header.as_str()).collect();
        assert_eq!(headers, ["@@ -3,4 +3,5 @@ func x()", "@@ -20 +21 @@"]);
        assert_eq!(
            file.hunks[0].lines,
            [
                line(LineKind::Context, Some(3), Some(3), "keep"),
                line(LineKind::Deleted, Some(4), None, "gone"),
                line(LineKind::Added, None, Some(4), "new one"),
                line(LineKind::Added, None, Some(5), "new two"),
                line(LineKind::Context, Some(5), Some(6), "tail"),
            ]
        );
        assert_eq!(
            file.hunks[1].lines,
            [
                line(LineKind::Deleted, Some(20), None, "single"),
                line(LineKind::Added, None, Some(21), "changed"),
            ]
        );
    }

    #[test]
    fn new_file_starts_at_line_one() {
        let files = parse_lines(&[
            "diff --git a/docs/new.md b/docs/new.md",
            "new file mode 100644",
            "index 0000000..abc",
            "--- /dev/null",
            "+++ b/docs/new.md",
            "@@ -0,0 +1,2 @@",
            "+one",
            "+two",
        ]);
        let file = &files[0];
        assert_eq!(file.status, FileStatus::Added);
        assert_eq!(file.path, "docs/new.md");
        assert_eq!(file.additions, 2);
        let new: Vec<_> = file.hunks[0].lines.iter().map(|l| l.new).collect();
        assert_eq!(new, [Some(1), Some(2)]);
    }

    #[test]
    fn deleted_file_keeps_its_path() {
        let files = parse_lines(&[
            "diff --git a/old.md b/old.md",
            "deleted file mode 100644",
            "--- a/old.md",
            "+++ /dev/null",
            "@@ -1,2 +0,0 @@",
            "-one",
            "-two",
        ]);
        assert_eq!(files[0].status, FileStatus::Deleted);
        assert_eq!(files[0].path, "old.md");
        assert_eq!(files[0].deletions, 2);
    }

    #[test]
    fn rename_uses_new_path_and_keeps_old_path() {
        let files = parse_lines(&[
            "diff --git a/src/Old.ts b/src/New.ts",
            "similarity index 80%",
            "rename from src/Old.ts",
            "rename to src/New.ts",
            "index 1..2 100644",
            "--- a/src/Old.ts",
            "+++ b/src/New.ts",
            "@@ -1 +1 @@",
            "-a",
            "+b",
        ]);
        assert_eq!(files[0].status, FileStatus::Renamed);
        assert_eq!(files[0].path, "src/New.ts");
        assert_eq!(files[0].old_path.as_deref(), Some("src/Old.ts"));
        assert_eq!(files[0].additions, 1);
    }

    #[test]
    fn pure_rename_has_no_hunks() {
        let files = parse_lines(&[
            "diff --git a/a.txt b/b.txt",
            "similarity index 100%",
            "rename from a.txt",
            "rename to b.txt",
        ]);
        let mut expected = FileDiff::new("b.txt", FileStatus::Renamed);
        expected.old_path = Some("a.txt".into());
        assert_eq!(files, [expected]);
    }

    #[test]
    fn binary_file_is_flagged_without_hunks() {
        let files = parse_lines(&[
            "diff --git a/logo.png b/logo.png",
            "new file mode 100644",
            "index 0000000..abc",
            "Binary files /dev/null and b/logo.png differ",
        ]);
        assert!(files[0].is_binary);
        assert_eq!(files[0].status, FileStatus::Added);
        assert_eq!(files[0].path, "logo.png");
        assert!(files[0].hunks.is_empty());
    }

    #[test]
    fn no_newline_marker_is_not_a_line() {
        let files = parse_lines(&[
            "diff --git a/a.txt b/a.txt",
            "--- a/a.txt",
            "+++ b/a.txt",
            "@@ -1 +1 @@",
            "-old",
            "\\ No newline at end of file",
            "+new",
            "\\ No newline at end of file",
        ]);
        assert_eq!(files[0].hunks[0].lines.len(), 2);
        assert_eq!((files[0].additions, files[0].deletions), (1, 1));
    }

    #[test]
    fn path_with_spaces_comes_from_the_header() {
        let files = parse_lines(&[
            "diff --git a/my docs/read me.md b/my docs/read me.md",
            "new file mode 100644",
            "--- /dev/null",
            "+++ b/my docs/read me.md",
            "@@ -0,0 +1 @@",
            "+x",
        ]);
        assert_eq!(files[0].path, "my docs/read me.md");
    }

    #[test]
    fn quoted_paths_are_unquoted() {
        let files = parse_lines(&[
            r#"diff --git "a/tab\there \"q\" \\ \303\251.txt" "b/tab\there \"q\" \\ \303\251.txt""#,
            "--- x",
            "+++ y",
            "@@ -1 +1 @@",
            "-a",
            "+b",
            "diff --git a/old name \"b/new\\nline\"",
            "similarity index 100%",
            r#"rename from old name"#,
            r#"rename to "new\nline""#,
            r#"diff --git "a/\a\b\f\r\v" "b/\a\b\f\r\v""#,
        ]);
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["tab\there \"q\" \\ \u{e9}.txt", "new\nline", "\u{7}\u{8}\u{c}\r\u{b}"]
        );
        assert_eq!(files[1].old_path.as_deref(), Some("old name"));
    }

    #[test]
    fn diff_paths_match_the_sidebar_paths() {
        use crate::testutil::{commit, init_repo, write};
        let dir = init_repo();
        let repo = dir.path();
        let names = [
            "plain.txt",
            "tab\there.txt",
            "quote\"d.txt",
            "back\\slash.txt",
            "caf\u{e9}.txt",
            "new\nline.txt",
            "a b/c d.txt",
        ];
        std::fs::create_dir(repo.join("a b")).unwrap();
        for name in names {
            write(repo, name, "one\n");
        }
        commit(repo, "base");
        for name in names {
            write(repo, name, "one\ntwo\n");
        }
        let sidebar: Vec<_> = crate::changes::load(repo, &Comparison::WorkingTree)
            .unwrap()
            .into_iter()
            .map(|change| change.path)
            .collect();
        let pane: Vec<_> = load(repo, &Comparison::WorkingTree)
            .unwrap()
            .into_iter()
            .map(|file| file.path)
            .collect();
        let mut sorted = sidebar.clone();
        sorted.sort();
        let mut expected: Vec<_> = names.map(String::from).to_vec();
        expected.sort();
        assert_eq!(sorted, expected);
        let mut pane_sorted = pane.clone();
        pane_sorted.sort();
        assert_eq!(pane_sorted, expected);
    }

    #[test]
    fn a_type_change_is_one_modified_entry() {
        let files = parse_lines(&[
            "diff --git a/link b/link",
            "deleted file mode 120000",
            "--- a/link",
            "+++ /dev/null",
            "@@ -1 +0,0 @@",
            "-target.txt",
            "\\ No newline at end of file",
            "diff --git a/link b/link",
            "new file mode 100644",
            "--- /dev/null",
            "+++ b/link",
            "@@ -0,0 +1,2 @@",
            "+now",
            "+a file",
            "diff --git a/other b/other",
            "--- a/other",
            "+++ b/other",
            "@@ -1 +1 @@",
            "-x",
            "+y",
        ]);
        let summary: Vec<_> = files
            .iter()
            .map(|f| (f.path.as_str(), f.status, f.additions, f.deletions, f.hunks.len()))
            .collect();
        assert_eq!(
            summary,
            [
                ("link", FileStatus::Modified, 2, 1, 2),
                ("other", FileStatus::Modified, 1, 1, 1)
            ]
        );
    }

    #[test]
    fn a_symlink_that_becomes_a_file_matches_the_sidebar() {
        use crate::testutil::{commit, init_repo, write};
        let dir = init_repo();
        let repo = dir.path();
        write(repo, "target.txt", "x\n");
        std::os::unix::fs::symlink("target.txt", repo.join("link")).unwrap();
        commit(repo, "base");
        std::fs::remove_file(repo.join("link")).unwrap();
        write(repo, "link", "now a file\n");
        let sidebar: Vec<_> = crate::changes::load(repo, &Comparison::WorkingTree)
            .unwrap()
            .into_iter()
            .map(|c| (c.path, c.status, c.additions, c.deletions))
            .collect();
        let pane: Vec<_> = load(repo, &Comparison::WorkingTree)
            .unwrap()
            .into_iter()
            .map(|f| (f.path, f.status, f.additions, f.deletions))
            .collect();
        assert_eq!(pane, sidebar);
    }

    #[test]
    fn pane_order_matches_the_sidebar_order_for_random_paths() {
        use crate::changes::FileChange;
        let parts = ["a", "A", "a-c", "a/z", "b", "z", "a.b", "_x", "B"];
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % bound as u64) as usize
        };
        for _ in 0..500 {
            let mut paths: Vec<String> = (0..1 + next(12))
                .map(|_| {
                    (0..1 + next(4))
                        .map(|_| parts[next(parts.len())])
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .collect();
            paths.sort();
            paths.dedup();
            let changes: Vec<FileChange> = paths
                .iter()
                .rev()
                .map(|path| FileChange {
                    path: path.clone(),
                    status: FileStatus::Modified,
                    additions: 0,
                    deletions: 0,
                    is_uncommitted: false,
                })
                .collect();
            let sidebar: Vec<String> = crate::tree::files(&crate::tree::build(&changes))
                .iter()
                .map(|file| file.path().to_string())
                .collect();
            let pane: Vec<String> =
                in_tree_order(paths.iter().map(|p| FileDiff::new(p, FileStatus::Modified)).collect())
                    .into_iter()
                    .map(|file| file.path)
                    .collect();
            assert_eq!(pane, sidebar, "paths {paths:?}");
        }
    }

    #[test]
    fn content_looking_like_headers_stays_content() {
        let files = parse_lines(&[
            "diff --git a/a.md b/a.md",
            "--- a/a.md",
            "+++ b/a.md",
            "@@ -1,2 +1,2 @@",
            "-diff --git a/x b/x",
            "+@@ -1 +1 @@",
            " --- a/y",
        ]);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].hunks.len(), 1);
        let texts: Vec<_> = files[0].hunks[0].lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["diff --git a/x b/x", "@@ -1 +1 @@", "--- a/y"]);
    }

    #[test]
    fn parses_several_files_in_order() {
        let files = parse_lines(&[
            "diff --git a/a b/a",
            "--- a/a",
            "+++ b/a",
            "@@ -1 +1 @@",
            "-x",
            "+y",
            "diff --git a/b b/b",
            "--- a/b",
            "+++ b/b",
            "@@ -1 +1 @@",
            "-x",
            "+y",
        ]);
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a", "b"]);
    }

    #[test]
    fn empty_input_has_no_files() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn puts_folders_first_and_ignores_case() {
        let paths = [
            "src/b.ts",
            "README.md",
            "docs/archive.md",
            "docs/Cache.md",
            "src/api/x.ts",
            "Zeta.md",
            "src/A.ts",
        ];
        let files = paths.iter().map(|p| FileDiff::new(p, FileStatus::Modified)).collect();
        let ordered: Vec<_> = in_tree_order(files).into_iter().map(|f| f.path).collect();
        assert_eq!(
            ordered,
            [
                "docs/archive.md",
                "docs/Cache.md",
                "src/api/x.ts",
                "src/A.ts",
                "src/b.ts",
                "README.md",
                "Zeta.md",
            ]
        );
    }

    #[test]
    fn tree_order_is_total_when_a_file_becomes_a_folder() {
        let mut paths: Vec<String> = vec!["a/b".into(), "a/b/c".into(), "a/a".into()];
        paths.extend((0..60).map(|i| format!("a/{}{}", (b'a' + (i % 26) as u8) as char, i)));
        let order = |input: &[String]| -> Vec<String> {
            let files = input.iter().map(|p| FileDiff::new(p, FileStatus::Modified)).collect();
            in_tree_order(files).into_iter().map(|f| f.path).collect()
        };
        let forward = order(&paths);
        paths.reverse();
        assert_eq!(order(&paths), forward);
        let b = forward.iter().position(|p| p == "a/b").unwrap();
        let bc = forward.iter().position(|p| p == "a/b/c").unwrap();
        let aa = forward.iter().position(|p| p == "a/a").unwrap();
        assert!(bc < aa && aa < b);
    }

    #[test]
    fn fingerprint_is_the_sha256_of_the_diff_content() {
        let mut file = FileDiff::new("a.txt", FileStatus::Modified);
        file.hunks.push(Hunk {
            header: "@@ -1 +1 @@".into(),
            lines: vec![line(LineKind::Added, None, Some(1), "x")],
        });
        let mut hasher = Sha256::new();
        hasher.update(b"M\0\0text\0@@ -1 +1 @@\0a\0x\0");
        let expected: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(file.fingerprint(), expected);
        assert_eq!(file.fingerprint().len(), 64);
        file.hunks[0].lines[0].text = "y".into();
        assert_ne!(file.fingerprint(), expected);
    }

    #[test]
    fn fingerprints_cover_only_the_asked_paths() {
        let files = [
            FileDiff::new("a", FileStatus::Modified),
            FileDiff::new("b", FileStatus::Modified),
        ];
        let wanted = std::collections::HashSet::from(["a".to_string()]);
        let result = fingerprints(&files, &wanted);
        assert_eq!(result.keys().collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn loads_a_real_diff_in_tree_order() {
        use crate::testutil::{commit, git, init_repo, write};
        let dir = init_repo();
        let repo = dir.path();
        std::fs::create_dir(repo.join("src")).unwrap();
        write(repo, "b.txt", "base\n");
        write(repo, "src/a.txt", "base\n");
        commit(repo, "base");
        write(repo, "b.txt", "base\nmore\n");
        write(repo, "src/a.txt", "other\n");
        git(repo, &["checkout", "-q", "-b", "topic"]);
        let files = load(repo, &Comparison::WorkingTree).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["src/a.txt", "b.txt"]);
        assert_eq!(files[1].additions, 1);
    }
}
