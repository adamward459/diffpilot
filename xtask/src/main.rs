mod checks;
mod features;
mod fixture;
mod perf;
mod pixels;
mod pr;
mod py_random;
mod select;
mod visual;
mod workspace;

use std::path::PathBuf;
use std::process::{Command, ExitCode};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

const USAGE: &str = "\
cargo xtask fmt                 format all Rust code
cargo xtask precommit           format, re-add the staged files, and check the staged Rust code
cargo xtask fixture [--force]   build the fixture repo in .fixtures/large (only if its generator changed)
cargo xtask unit [feature...] [--force]   cargo test for the features (default: all)
cargo xtask clippy [crate...] [--force]   cargo clippy --all-targets -- -D warnings
cargo xtask flows               headless user flows through the whole app (gpui-app/tests/flows/)
cargo xtask perf [feature...] [--base REF] [--force]
                                run each bench once and compare its time and memory metrics with
                                baselines/bench.json on the PR base ref (or REF); skips on a busy machine
cargo xtask baseline [feature...] [--force]
                                run each bench once and write its part of baselines/bench.json
cargo xtask visual [feature...] [--compare [--base REF]] [--dry-run]
                                capture each scene of the features into screenshots/<feature>/;
                                --compare pixel-diffs each image with the base ref's copy
cargo xtask pr-screenshots [feature...]
                                capture and push the scenes of the features the open PR touches,
                                and put the images in its body
cargo xtask related             print the commands that match the current changes
cargo xtask features            print the feature table and the files no feature lists";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let force = args.iter().any(|a| a == "--force");
    let rest = args.get(1..).unwrap_or_default();
    let outcome = match args.first().map(String::as_str) {
        Some("fmt") => fmt(),
        Some("precommit") => precommit(),
        Some("fixture") => fixture::ensure(force),
        Some("unit") => checks::unit(rest),
        Some("clippy") => checks::clippy(rest),
        Some("flows") => checks::flows(),
        Some("perf") => perf::perf(rest),
        Some("baseline") => perf::baseline(rest),
        Some("visual") => visual::visual(rest),
        Some("pr-screenshots") => pr::pr_screenshots(rest),
        Some("related") => checks::related(),
        Some("features") => checks::features(),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(64);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Found from the current folder, not `CARGO_MANIFEST_DIR`: worktrees share one build dir, so one xtask binary
/// serves every worktree and a path baked in at build time points at whichever worktree built it.
fn root() -> PathBuf {
    let start = std::env::current_dir().expect("a current folder");
    start
        .ancestors()
        .find(|dir| dir.join("xtask").join("Cargo.toml").is_file())
        .unwrap_or_else(|| panic!("{} is not inside the diffpilot workspace", start.display()))
        .to_path_buf()
}

fn run(cmd: &mut Command) -> Result<String> {
    let out = cmd.current_dir(root()).output()?;
    if !out.status.success() {
        return Err(format!("{cmd:?} failed:\n{}", String::from_utf8_lossy(&out.stderr).trim()).into());
    }
    Ok(String::from_utf8(out.stdout)?)
}

fn git(args: &[&str]) -> Result<Vec<String>> {
    Ok(run(Command::new("git").args(args))?
        .lines()
        .map(str::to_owned)
        .collect())
}

fn fmt() -> Result {
    run(Command::new("cargo").args(["fmt", "--all"])).map(drop)
}

fn precommit() -> Result {
    let staged = git(&["diff", "--cached", "--name-only", "--diff-filter=ACMR"])?;
    fmt()?;
    if !staged.is_empty() {
        let mut add = vec!["add", "--"];
        add.extend(staged.iter().map(String::as_str));
        git(&add)?;
    }
    check_bin_paths()
}

// With the shared build dir, env! bakes in the path of whichever worktree compiled the test.
fn check_bin_paths() -> Result {
    let pattern = concat!("env", "!(\"CARGO_BIN_EXE_");
    let out = Command::new("git")
        .args(["grep", "-l", "-F", pattern, "--", "*.rs"])
        .current_dir(root())
        .output()?;
    let hits = String::from_utf8(out.stdout)?;
    if hits.trim().is_empty() {
        return Ok(());
    }
    Err(format!(
        "{}: read std::env::var(\"CARGO_BIN_EXE_<name>\") at run time instead of {pattern}<name>\")",
        hits.split_whitespace().collect::<Vec<_>>().join(", ")
    )
    .into())
}

pub fn parse_option(rest: &[String], name: &str) -> Result<(Option<String>, Vec<String>)> {
    let mut out = vec![];
    let mut value = None;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        if arg == name {
            value = Some(it.next().ok_or_else(|| format!("{name} needs a value"))?.clone());
        } else {
            out.push(arg.clone());
        }
    }
    Ok((value, out))
}

pub fn take_flag(rest: &[String], name: &str) -> (bool, Vec<String>) {
    (
        rest.iter().any(|a| a == name),
        rest.iter().filter(|a| *a != name).cloned().collect(),
    )
}

pub fn resolve_base(explicit: Option<String>) -> String {
    if let Some(base) = explicit {
        return base;
    }
    let name = run(Command::new("gh").args(["pr", "view", "--json", "baseRefName", "-q", ".baseRefName"]))
        .map(|n| n.trim().to_owned())
        .unwrap_or_else(|_| "main".into());
    let _ = Command::new("git")
        .args(["fetch", "-q", "origin", &name])
        .current_dir(root())
        .output();
    format!("origin/{name}")
}
