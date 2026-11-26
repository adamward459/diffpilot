use crate::features::{FEATURES, Feature, TestSel, Usage, listed, resolve};
use crate::select::select;
use crate::workspace::{Crate, ROOT_FILES, Workspace};
use crate::{Result, git, root, run};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, TryLockError};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn passes_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")?;
    Ok(PathBuf::from(home).join("Library/Caches/diffpilot/passes"))
}

pub fn tracked_files() -> Result<Vec<String>> {
    let out = run(Command::new("git").args(["ls-files", "-z", "-co", "--exclude-standard"]))?;
    let mut files: Vec<String> = out.split('\0').filter(|f| !f.is_empty()).map(str::to_owned).collect();
    files.sort();
    files.dedup();
    Ok(files)
}

fn digest(check: &str, name: &str, files: &[&String]) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(format!("{check}\0{name}\0"));
    hash.update(run(Command::new("rustc").arg("-vV"))?);
    let root = root();
    for file in ROOT_FILES.iter().copied().chain(files.iter().map(|f| f.as_str())) {
        hash.update(file);
        hash.update(b"\0");
        match fs::read(root.join(file)) {
            Ok(bytes) => hash.update(bytes),
            Err(_) => hash.update(b"\0missing"),
        }
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn feature_digest(ws: &Workspace, usage: &Usage, files: &[String], feature: &Feature) -> Result<String> {
    let own: BTreeSet<&str> = feature.packages().collect();
    let deps: BTreeSet<&str> = own
        .iter()
        .flat_map(|p| ws.closure(p))
        .filter(|p| !own.contains(p))
        .collect();
    let crates_of = |names: &BTreeSet<&str>| -> Vec<&Crate> { names.iter().filter_map(|n| ws.get(n)).collect() };
    let (own, deps) = (crates_of(&own), crates_of(&deps));
    let index = FEATURES
        .iter()
        .position(|f| f.name == feature.name)
        .expect("features come from FEATURES");
    let used: Vec<&Feature> = usage.used_by(index).map(|i| &FEATURES[i]).collect();
    let chosen: Vec<&String> = files
        .iter()
        .filter(|f| {
            feature.lists(f)
                || used.iter().any(|u| u.lists(f))
                || (!listed(f)
                    && (own.iter().any(|c| c.contains(f))
                        || deps.iter().any(|c| c.contains(f) && c.builds_into_dependents(f))))
        })
        .collect();
    digest("unit", feature.name, &chosen)
}

fn crate_digest(ws: &Workspace, files: &[String], krate: &Crate) -> Result<String> {
    let deps: Vec<&Crate> = ws
        .closure(&krate.package)
        .into_iter()
        .filter(|p| *p != krate.package)
        .filter_map(|p| ws.get(p))
        .collect();
    let chosen: Vec<&String> = files
        .iter()
        .filter(|f| krate.contains(f) || deps.iter().any(|d| d.contains(f) && d.builds_into_dependents(f)))
        .collect();
    digest("clippy", &krate.package, &chosen)
}

fn record(check: &str, name: &str, digest: &str) -> Result<PathBuf> {
    Ok(passes_dir()?.join(format!("{check}-{name}-{digest}")))
}

struct Todo {
    name: String,
    digest: String,
}

fn unpassed(check: &str, digests: Vec<(String, String)>, force: bool) -> Result<Vec<Todo>> {
    let mut todo = vec![];
    for (name, digest) in digests {
        if !force && record(check, &name, &digest)?.exists() {
            eprintln!("  {check} {name}: skipped (passed at {})", &digest[..12]);
        } else {
            todo.push(Todo { name, digest });
        }
    }
    Ok(todo)
}

fn record_passes(check: &str, todo: &[Todo]) -> Result {
    fs::create_dir_all(passes_dir()?)?;
    for t in todo {
        File::create(record(check, &t.name, &t.digest)?)?;
    }
    Ok(())
}

// Worktrees share one build dir, so a workspace crate has the same test binary path in each.
// Cargo frees its own lock before it runs tests, so another worktree could replace the binary.
fn build_dir_lock(ws: &Workspace) -> Result<File> {
    fs::create_dir_all(&ws.build_dir)?;
    let lock = File::create(ws.build_dir.join(".diffpilot-test.lock"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            eprintln!("waiting for another worktree's tests in {}", ws.build_dir.display());
            lock.lock()?;
        }
        Err(TryLockError::Error(e)) => return Err(e.into()),
    }
    Ok(lock)
}

fn cargo(args: &[String]) -> Result<bool> {
    eprintln!("+ cargo {}", args.join(" "));
    Ok(Command::new("cargo").args(args).current_dir(root()).status()?.success())
}

pub fn flows() -> Result {
    let ws = Workspace::load()?;
    let _lock = build_dir_lock(&ws)?;
    if cargo(&args(&["test", "-p", "diffpilot", "--test", "flows"]))? {
        Ok(())
    } else {
        Err("flows failed".into())
    }
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

struct Exe {
    package: String,
    path: PathBuf,
    folder: PathBuf,
}

#[derive(Default)]
struct Plan {
    modules: BTreeMap<&'static str, BTreeSet<&'static str>>,
    targets: BTreeSet<&'static str>,
    packages: BTreeSet<&'static str>,
}

impl Plan {
    fn of(features: &[&Feature]) -> Self {
        let mut plan = Plan::default();
        for sel in features.iter().flat_map(|f| f.tests) {
            plan.packages.insert(sel.package());
            match sel {
                TestSel::Lib { package, module } => {
                    plan.modules.entry(package).or_default().insert(module);
                }
                TestSel::Target { test, .. } => {
                    plan.targets.insert(test);
                }
            }
        }
        plan
    }

    // Every call names the same packages, so Cargo resolves features once and rebuilds nothing between calls.
    fn command(&self, verb: &[&str], targets: &[String]) -> Vec<String> {
        let mut out = args(verb);
        self.packages.iter().for_each(|p| out.extend(args(&["-p", p])));
        out.extend(targets.iter().cloned());
        out
    }

    // Cargo refuses --lib when none of the -p packages has a library, as with xtask alone.
    fn lib_targets(&self, ws: &Workspace) -> Vec<String> {
        let has_lib = ws
            .crates
            .iter()
            .any(|c| c.has_lib && self.packages.contains(c.package.as_str()));
        if has_lib {
            args(&["--lib", "--bins"])
        } else {
            args(&["--bins"])
        }
    }

    fn test_targets(&self) -> Vec<String> {
        self.targets.iter().flat_map(|t| args(&["--test", t])).collect()
    }

    fn build(&self, ws: &Workspace) -> Result<Vec<Exe>> {
        let mut targets = self.lib_targets(ws);
        targets.extend(self.test_targets());
        let build = self.command(
            &["test", "--no-run", "--message-format=json-render-diagnostics"],
            &targets,
        );
        eprintln!("+ cargo {}", build.join(" "));
        let mut child = Command::new("cargo")
            .args(&build)
            .current_dir(root())
            .stdout(Stdio::piped())
            .spawn()?;
        let mut exes = vec![];
        for line in BufReader::new(child.stdout.take().ok_or("no cargo output")?).lines() {
            let Ok(message) = serde_json::from_str::<Value>(&line?) else {
                continue;
            };
            let unit_test = message["reason"] == "compiler-artifact" && message["profile"]["test"] == true;
            let kinds = &message["target"]["kind"];
            let unit_kind = kinds
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == "lib" || k == "bin"));
            if let (true, true, Some(path), Some(manifest)) = (
                unit_test,
                unit_kind,
                message["executable"].as_str(),
                message["manifest_path"].as_str(),
            ) && let Some(krate) = ws
                .crates
                .iter()
                .find(|c| root().join(&c.folder).join("Cargo.toml") == Path::new(manifest))
            {
                exes.push(Exe {
                    package: krate.package.clone(),
                    path: path.into(),
                    folder: root().join(&krate.folder),
                });
            }
        }
        if !child.wait()?.success() {
            return Err("the test build failed".into());
        }
        Ok(exes)
    }

    // A libtest filter matches anywhere in a test name, so `review::` would also pick `github::review::...`.
    // Listing the names first lets a module match only at the start.
    fn runs(&self, ws: &Workspace, exes: &[Exe]) -> Result<Vec<Vec<String>>> {
        let mut names = vec![];
        for exe in exes {
            let Some(modules) = self.modules.get(exe.package.as_str()) else {
                continue;
            };
            let listed = run(Command::new(&exe.path)
                .current_dir(&exe.folder)
                .args(["--list", "--format", "terse"]))?;
            names.extend(
                listed
                    .lines()
                    .filter_map(|l| l.strip_suffix(": test"))
                    .filter(|name| modules.iter().any(|m| name.starts_with(m)))
                    .map(str::to_owned),
            );
        }
        let mut out = vec![];
        if !names.is_empty() {
            let mut lib = self.command(&["test", "--no-fail-fast"], &self.lib_targets(ws));
            lib.extend(args(&["--", "--exact"]));
            lib.extend(names);
            out.push(lib);
        }
        if !self.targets.is_empty() {
            out.push(self.command(&["test", "--no-fail-fast"], &self.test_targets()));
        }
        Ok(out)
    }
}

fn parse(rest: &[String]) -> (Vec<&str>, bool) {
    let force = rest.iter().any(|a| a == "--force");
    (
        rest.iter().map(String::as_str).filter(|a| *a != "--force").collect(),
        force,
    )
}

pub fn unit(rest: &[String]) -> Result {
    let (all, rest) = crate::take_flag(rest, "--all");
    let (names, force) = parse(&rest);
    if names.is_empty() && !all {
        return Err(
            "name the features to test: run `cargo xtask related` and use the `unit` line it prints, \
                    or pass --all to test every feature"
                .into(),
        );
    }
    let features = resolve(&names)?;
    let ws = Workspace::load()?;
    let files = tracked_files()?;
    let usage = Usage::scan(&ws, &files);
    let digests = features
        .iter()
        .map(|f| Ok((f.name.to_owned(), feature_digest(&ws, &usage, &files, f)?)))
        .collect::<Result<Vec<_>>>()?;
    let todo = unpassed("unit", digests, force)?;
    let run: Vec<&Feature> = features
        .into_iter()
        .filter(|f| todo.iter().any(|t| t.name == f.name))
        .collect();
    if run.is_empty() {
        return Ok(());
    }
    let plan = Plan::of(&run);
    let _lock = build_dir_lock(&ws)?;
    let exes = plan.build(&ws)?;
    let mut ok = true;
    for command in plan.runs(&ws, &exes)? {
        ok &= cargo(&command)?;
    }
    if !ok {
        return Err("unit tests failed".into());
    }
    record_passes("unit", &todo)
}

pub fn clippy(rest: &[String]) -> Result {
    let (names, force) = parse(rest);
    let ws = Workspace::load()?;
    let chosen: Vec<&Crate> = if names.is_empty() {
        ws.crates.iter().collect()
    } else {
        names.iter().map(|n| ws.resolve(n)).collect::<Result<_>>()?
    };
    let files = tracked_files()?;
    let digests = chosen
        .iter()
        .map(|c| Ok((c.package.clone(), crate_digest(&ws, &files, c)?)))
        .collect::<Result<Vec<_>>>()?;
    let todo = unpassed("clippy", digests, force)?;
    if todo.is_empty() {
        return Ok(());
    }
    let mut command = args(&["clippy"]);
    todo.iter().for_each(|t| command.extend(args(&["-p", &t.name])));
    command.extend(args(&["--all-targets", "--", "-D", "warnings"]));
    if !cargo(&command)? {
        return Err("clippy failed".into());
    }
    record_passes("clippy", &todo)
}

fn base() -> Result<String> {
    if let Ok(lines) = git(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"])
        && let Some(upstream) = lines.into_iter().next()
    {
        return Ok(upstream);
    }
    let name = run(Command::new("gh").args(["pr", "view", "--json", "baseRefName", "-q", ".baseRefName"]))
        .map(|n| n.trim().to_owned())
        .unwrap_or_else(|_| "main".into());
    let _ = Command::new("git")
        .args(["fetch", "-q", "origin", &name])
        .current_dir(root())
        .output();
    let base = format!("origin/{name}");
    Ok(git(&["merge-base", "HEAD", &base])?
        .into_iter()
        .next()
        .ok_or("no merge base")?)
}

pub fn changed_files(base: &str) -> Result<Vec<String>> {
    let mut files: BTreeSet<String> = git(&["diff", "--name-only", base])?.into_iter().collect();
    files.extend(git(&["ls-files", "-o", "--exclude-standard"])?);
    Ok(files.into_iter().collect())
}

pub fn related() -> Result {
    let ws = Workspace::load()?;
    let files = changed_files(&base()?)?;
    let touched = ws.touched(&files);
    if touched.is_empty() {
        println!("No changes in crates.");
        return Ok(());
    }
    let all = tracked_files()?;
    let usage = Usage::scan(&ws, &all);
    let picks = select(&ws, &usage, &files);
    let folders = |names: &[&str]| -> Vec<String> {
        ws.crates
            .iter()
            .filter(|c| names.contains(&c.package.as_str()))
            .map(|c| c.folder.clone())
            .collect()
    };
    println!("Touched features:");
    for p in &picks {
        let reached: Vec<_> = p.packages.iter().copied().collect();
        println!("  {} (tests reached in {})", p.feature.name, reached.join(" "));
    }
    let unit_digests = picks
        .iter()
        .map(|p| Ok((p.feature.name.to_owned(), feature_digest(&ws, &usage, &all, p.feature)?)))
        .collect::<Result<Vec<_>>>()?;
    let unit = unpassed("unit", unit_digests, false)?;
    if !unit.is_empty() {
        println!(
            "  cargo xtask unit {}",
            unit.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(" ")
        );
    }
    let crate_digests = touched
        .iter()
        .filter_map(|p| ws.get(p))
        .map(|c| Ok((c.package.clone(), crate_digest(&ws, &all, c)?)))
        .collect::<Result<Vec<_>>>()?;
    let clippy = unpassed("clippy", crate_digests, false)?;
    if !clippy.is_empty() {
        let names: Vec<&str> = clippy.iter().map(|t| t.name.as_str()).collect();
        println!("  cargo xtask clippy {}", folders(&names).join(" "));
    }
    let built = ws.built(&files);
    if built.contains("diffpilot") {
        println!("  cargo xtask flows");
        let benched: Vec<&str> = picks
            .iter()
            .filter(|p| p.feature.bench.is_some())
            .map(|p| p.feature.name)
            .collect();
        let scened: Vec<&str> = picks
            .iter()
            .filter(|p| !p.feature.scenes.is_empty())
            .map(|p| p.feature.name)
            .collect();
        if !benched.is_empty() || !scened.is_empty() {
            println!("These open windows. Run them only when the user asks:");
        }
        if !benched.is_empty() {
            println!("  cargo xtask perf {}", benched.join(" "));
        }
        if !scened.is_empty() {
            println!("  cargo xtask visual {} --compare", scened.join(" "));
        }
        println!("Before you open or update the PR (then commit baselines/bench.json):");
        println!("  cargo xtask baseline");
        println!("After you open or update the PR:");
        println!("  cargo xtask pr-screenshots");
    }
    Ok(())
}

pub fn features() -> Result {
    let ws = Workspace::load()?;
    let files = tracked_files()?;
    let usage = Usage::scan(&ws, &files);
    for (i, f) in FEATURES.iter().enumerate() {
        println!("{}", f.name);
        let users: Vec<_> = usage.users(i).iter().map(|&u| FEATURES[u].name).collect();
        println!(
            "  used by  {}",
            if users.is_empty() {
                "-".to_owned()
            } else {
                users.join(", ")
            }
        );
        for p in f.paths {
            println!("  path  {p}");
        }
        for t in f.tests {
            match t {
                TestSel::Lib { package, module } => println!("  test  {package} lib {module:?}"),
                TestSel::Target { package, test } => println!("  test  {package} --test {test}"),
            }
        }
    }
    let unlisted: Vec<String> = files
        .into_iter()
        .filter(|f| (f.starts_with("core/src/") || f.starts_with("gpui-app/src/")) && !listed(f))
        .collect();
    println!(
        "\nUnlisted files under core/src and gpui-app/src (a change selects the features that test, name, or hold a file of their top-level module):"
    );
    unlisted.iter().for_each(|f| println!("  {f}"));
    Ok(())
}
