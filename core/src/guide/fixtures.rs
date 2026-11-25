use crate::diff::{self, FileDiff};
use crate::testutil::{commit, git, init_repo, write};
use std::path::Path;
use tempfile::TempDir;

pub const SEND_BASE: &str =
    "// delivery\nexport function send(id: string) {\n  log(id);\n  deliver(id);\n  audit(id);\n}\n";
pub const SEND_NEW: &str = "import { startTimer } from \"./timer\";\n// delivery\nexport function send(id: string) {\n  log(id);\n  startTimer(id);\n  audit(id);\n}\n";
pub const TIMER: &str = "export function startTimer(id: string) {\n  return setTimeout(() => deliver(id), 5000);\n}\n";

/// A repo with one commit and three staged changes: `src/send.ts` (modified), `src/timer.ts` (new) and
/// `notes.md` (modified).
pub fn repo() -> TempDir {
    let dir = init_repo();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    write(dir.path(), "src/send.ts", SEND_BASE);
    write(dir.path(), "notes.md", "# notes\n");
    commit(dir.path(), "base");
    write(dir.path(), "src/send.ts", SEND_NEW);
    write(dir.path(), "src/timer.ts", TIMER);
    write(dir.path(), "notes.md", "# notes\n- undo send\n");
    git(dir.path(), &["add", "-A"]);
    dir
}

pub fn load(repo: &Path) -> Vec<FileDiff> {
    diff::load_revisions(repo, &["HEAD".to_string()]).unwrap()
}

pub fn file<'a>(files: &'a [FileDiff], path: &str) -> &'a FileDiff {
    files.iter().find(|file| file.path == path).unwrap()
}
