use crate::py_random::PyRandom;
use crate::{Result, root, run};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GENERATED_COUNT: usize = 1988;
const CONTEXT_LINES: usize = 3;
const LONG_LINE_PATH: &str = "src/components/inbox/MessageRow.tsx";
const SOURCES: [&str; 2] = [include_str!("fixture.rs"), include_str!("py_random.rs")];

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Added,
    Modified,
    Deleted,
    Renamed,
}

struct Spec {
    path: String,
    status: Status,
    additions: usize,
    deletions: usize,
    viewed: bool,
    uncommitted: bool,
}

impl Spec {
    fn old_path(&self) -> String {
        match self.path.rsplit_once('/') {
            Some((folder, name)) => format!("{folder}/Old{name}"),
            None => format!("Old{}", self.path),
        }
    }
}

pub fn repo() -> PathBuf {
    root().join(".fixtures").join("large")
}

pub fn viewed_seed() -> PathBuf {
    root().join(".fixtures").join("large-viewed.json")
}

pub fn ensure(force: bool) -> Result {
    let repo = repo();
    let stamp = repo.join(".git").join("diffpilot-fixture-version");
    let version = version();
    if !force && fs::read_to_string(&stamp).is_ok_and(|v| v == version) {
        return Ok(());
    }
    let taken: HashSet<String> = design().into_iter().map(|s| s.path).collect();
    let specs: Vec<Spec> = design().into_iter().chain(generated(GENERATED_COUNT, taken)).collect();
    build(&repo, &specs)?;
    fs::write(&stamp, version)?;
    eprintln!("Built {} with {} changed files", repo.display(), specs.len());
    Ok(())
}

fn version() -> String {
    let mut digest = Sha256::new();
    for source in SOURCES {
        digest.update(source);
    }
    format!("{:x}", digest.finalize())
}

fn design() -> Vec<Spec> {
    use Status::*;
    let spec = |path: &str, status, additions, deletions, viewed, uncommitted| Spec {
        path: path.into(),
        status,
        additions,
        deletions,
        viewed,
        uncommitted,
    };
    vec![
        spec("docs/archive.md", Added, 19, 0, false, false),
        spec("src/api/archive.ts", Modified, 38, 6, true, false),
        spec("src/api/client.ts", Modified, 4, 1, true, false),
        spec("src/components/inbox/ArchiveToast.tsx", Added, 48, 0, false, false),
        spec("src/components/inbox/MessageRow.tsx", Modified, 23, 15, false, false),
        spec("src/hooks/useArchiveUndo.ts", Added, 64, 0, false, true),
        spec("src/lib/queryKeys.ts", Modified, 3, 1, true, false),
        spec("src/types/message.ts", Modified, 5, 0, false, false),
        spec("tests/api/archive.test.ts", Modified, 31, 4, false, true),
        spec("tests/hooks/useArchiveUndo.test.ts", Added, 112, 0, false, false),
        spec("package.json", Modified, 1, 1, false, false),
        spec("pnpm-lock.yaml", Modified, 21, 8, false, false),
    ]
}

// Every rng call and its order matches the Python script that made the committed fixture.
fn generated(count: usize, mut seen: HashSet<String>) -> Vec<Spec> {
    let mut rng = PyRandom::new(0x9E37_79B9);
    let roots = [
        "src", "src", "src", "src", "tests", "tests", "docs", "packages", "scripts",
    ];
    let areas = [
        "api",
        "components",
        "hooks",
        "lib",
        "types",
        "utils",
        "store",
        "features/auth",
        "views",
        "models",
    ];
    let subs = ["", "", "", "inbox", "compose", "settings", "shared/forms", "legacy"];
    let words = [
        "Message", "Inbox", "Query", "Client", "Thread", "Label", "Filter", "Draft", "Sync", "Cache", "Session",
    ];
    let kinds = [
        "", "Row", "List", "View", "Provider", "Store", "Service", "Helper", "Config",
    ];
    let extensions = ["ts", "ts", "tsx", "tsx", "swift", "swift", "md", "json", "yaml"];

    let mut specs = Vec::with_capacity(count);
    while specs.len() < count {
        let root = rng.choice(&roots);
        let parts: Vec<&str> = if root == "docs" {
            vec![root]
        } else {
            let area = rng.choice(&areas);
            let sub = rng.choice(&subs);
            vec![root, area, sub]
        };
        let ext = if root == "docs" { "md" } else { rng.choice(&extensions) };
        let word = rng.choice(&words);
        let base = format!("{word}{}", rng.choice(&kinds));
        let folder: Vec<&str> = parts.into_iter().filter(|p| !p.is_empty()).collect();
        let join = |name: String| {
            folder
                .iter()
                .copied()
                .chain([name.as_str()])
                .collect::<Vec<_>>()
                .join("/")
        };
        let mut path = join(format!("{base}.{ext}"));
        let mut suffix = 2;
        while seen.contains(&path) {
            path = join(format!("{base}{suffix}.{ext}"));
            suffix += 1;
        }
        seen.insert(path.clone());
        let roll = rng.randrange(100);
        let status = match roll {
            0..60 => Status::Modified,
            60..82 => Status::Added,
            82..90 => Status::Deleted,
            _ => Status::Renamed,
        };
        let size = 1 + rng.randrange(40) as usize;
        let additions = if status == Status::Deleted { 0 } else { size };
        let deletions = match status {
            Status::Added => 0,
            Status::Deleted => 1 + rng.randrange(14) as usize,
            _ => rng.randrange(15) as usize,
        };
        let viewed = rng.randrange(100) < 25;
        let uncommitted = rng.randrange(100) < 12;
        specs.push(Spec {
            path,
            status,
            additions,
            deletions,
            viewed,
            uncommitted,
        });
        if rng.randrange(100) < 15 {
            rng.randrange(5);
        }
    }
    specs
}

fn lines(spec: &Spec, kind: &str, count: usize) -> Vec<String> {
    let mut out: Vec<String> = (0..count).map(|i| format!("{} {kind} {i}", spec.path)).collect();
    if spec.path == LONG_LINE_PATH && kind == "new" && !out.is_empty() {
        out[0] += &" the quick brown fox jumps over the lazy dog".repeat(12);
    }
    out
}

// Rename detection needs the old and new file to be mostly the same.
fn shared_lines(spec: &Spec) -> Vec<String> {
    lines(spec, "shared", 2 * spec.additions.max(spec.deletions) + 10)
}

fn base_content(spec: &Spec) -> Option<Vec<String>> {
    Some(match spec.status {
        Status::Added => return None,
        Status::Renamed => [shared_lines(spec), lines(spec, "old", spec.deletions)].concat(),
        Status::Deleted => lines(spec, "old", spec.deletions),
        Status::Modified => [
            lines(spec, "context", CONTEXT_LINES),
            lines(spec, "old", spec.deletions),
        ]
        .concat(),
    })
}

fn head_content(spec: &Spec) -> Option<Vec<String>> {
    Some(match spec.status {
        Status::Deleted => return None,
        Status::Renamed => [shared_lines(spec), lines(spec, "new", spec.additions)].concat(),
        Status::Added => lines(spec, "new", spec.additions),
        Status::Modified => [
            lines(spec, "context", CONTEXT_LINES),
            lines(spec, "new", spec.additions),
        ]
        .concat(),
    })
}

fn write(path: &Path, content: Option<Vec<String>>) -> Result {
    let Some(content) = content else {
        return match fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        };
    };
    fs::create_dir_all(path.parent().expect("fixture files live in the repo"))?;
    fs::write(path, content.iter().map(|line| format!("{line}\n")).collect::<String>())?;
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Result {
    run(Command::new("git").arg("-C").arg(repo).args(args)).map(drop)
}

fn apply<'a>(repo: &Path, specs: impl Iterator<Item = &'a Spec>) -> Result {
    for spec in specs {
        if spec.status == Status::Renamed {
            fs::remove_file(repo.join(spec.old_path()))?;
        }
        write(&repo.join(&spec.path), head_content(spec))?;
    }
    Ok(())
}

fn build(repo: &Path, specs: &[Spec]) -> Result {
    if repo.exists() {
        fs::remove_dir_all(repo)?;
    }
    fs::create_dir_all(repo)?;
    git(repo, &["init", "-q", "-b", "main"])?;
    git(repo, &["config", "user.name", "diffpilot fixture"])?;
    git(repo, &["config", "user.email", "fixture@diffpilot.invalid"])?;
    for spec in specs {
        let path = if spec.status == Status::Renamed {
            spec.old_path()
        } else {
            spec.path.clone()
        };
        write(&repo.join(path), base_content(spec))?;
    }
    git(repo, &["add", "-A"])?;
    git(repo, &["commit", "-q", "-m", "base"])?;
    git(repo, &["checkout", "-q", "-b", "feature"])?;
    apply(repo, specs.iter().filter(|s| !s.uncommitted))?;
    git(repo, &["add", "-A"])?;
    git(repo, &["commit", "-q", "-m", "feature"])?;
    apply(repo, specs.iter().filter(|s| s.uncommitted))?;
    git(repo, &["add", "-A"])?;
    let mut viewed: Vec<&str> = specs.iter().filter(|s| s.viewed).map(|s| s.path.as_str()).collect();
    viewed.sort();
    fs::write(viewed_seed(), serde_json::to_string_pretty(&viewed)? + "\n")?;
    Ok(())
}
