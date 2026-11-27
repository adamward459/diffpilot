use crate::workspace::Workspace;
use crate::{Result, git, parse_option, take_flag};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

const DEFAULT_MAX_GB: u64 = 50;
const KB_PER_GB: u64 = 1024 * 1024;
// Cargo offers no way to ask whether a build runs in a dir, so a recent write stands in for one.
const BUSY: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    None,
    OtherWorktree,
    ThisWorktree,
}

#[derive(Debug)]
struct BuildDir {
    path: PathBuf,
    kb: u64,
    last_used: SystemTime,
    owner: Owner,
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Keep,
    DeleteOrphan,
    DeleteOverLimit,
}

pub fn cache(rest: &[String]) -> Result {
    let (max_gb, rest) = parse_option(rest, "--max-gb")?;
    let (dry_run, rest) = take_flag(&rest, "--dry-run");
    if let Some(unknown) = rest.first() {
        return Err(format!("unknown argument {unknown:?}").into());
    }
    let max_gb = match max_gb {
        Some(text) => text
            .parse()
            .map_err(|_| format!("--max-gb {text:?} is not a whole number"))?,
        None => DEFAULT_MAX_GB,
    };

    let this = Workspace::load()?.build_dir;
    let root = build_root(&this)?;
    let live = live_build_dirs()?;
    let mut dirs = vec![];
    for path in build_dirs(&root)? {
        let owner = match () {
            () if path == this => Owner::ThisWorktree,
            () if live.contains(&path) => Owner::OtherWorktree,
            () => Owner::None,
        };
        dirs.push(BuildDir {
            kb: disk_kb(&path)?,
            last_used: last_used(&path),
            owner,
            path,
        });
    }

    let verdicts = plan(&dirs, max_gb * KB_PER_GB, SystemTime::now());
    let mut kept = 0;
    for (dir, verdict) in dirs.iter().zip(&verdicts) {
        let label = match (verdict, dir.owner) {
            (Verdict::DeleteOrphan, _) => "delete (no worktree)",
            (Verdict::DeleteOverLimit, _) => "delete (over the limit, least recently used)",
            (Verdict::Keep, Owner::None) => unreachable!("an orphan is always deleted"),
            (Verdict::Keep, Owner::ThisWorktree) => "keep (this worktree)",
            (Verdict::Keep, Owner::OtherWorktree) => "keep",
        };
        println!("{:>6.1} GB  {label}  {}", gb(dir.kb), dir.path.display());
        if *verdict == Verdict::Keep {
            kept += dir.kb;
            continue;
        }
        if !dry_run {
            fs::remove_dir_all(&dir.path)?;
            if let Some(parent) = dir.path.parent().filter(|p| *p != root) {
                let _ = fs::remove_dir(parent);
            }
        }
    }
    let freed = dirs.iter().map(|d| d.kb).sum::<u64>() - kept;
    let verb = if dry_run { "would free" } else { "freed" };
    println!("{verb} {:.1} GB, {:.1} GB left, limit {max_gb} GB", gb(freed), gb(kept));
    if kept > max_gb * KB_PER_GB {
        println!("still over the limit: the dirs left belong to this worktree or were written in the last hour");
    }
    Ok(())
}

fn plan(dirs: &[BuildDir], max_kb: u64, now: SystemTime) -> Vec<Verdict> {
    let mut verdicts: Vec<Verdict> = dirs
        .iter()
        .map(|d| match d.owner {
            Owner::None => Verdict::DeleteOrphan,
            _ => Verdict::Keep,
        })
        .collect();
    let mut total: u64 = dirs.iter().filter(|d| d.owner != Owner::None).map(|d| d.kb).sum();
    let mut oldest_first: Vec<usize> = (0..dirs.len())
        .filter(|&i| dirs[i].owner == Owner::OtherWorktree)
        .filter(|&i| now.duration_since(dirs[i].last_used).is_ok_and(|idle| idle >= BUSY))
        .collect();
    oldest_first.sort_by_key(|&i| dirs[i].last_used);
    for i in oldest_first {
        if total <= max_kb {
            break;
        }
        verdicts[i] = Verdict::DeleteOverLimit;
        total -= dirs[i].kb;
    }
    verdicts
}

// `.cargo/config.toml` puts each worktree's build dir at <cargo home>/build/diffpilot-<2 hex>/<hex>.
// Worktrees on a commit before that layout share <cargo home>/build/diffpilot.
fn build_root(this: &Path) -> Result<PathBuf> {
    this.ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "build"))
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            format!(
                "{} is not under a `build` folder; see .cargo/config.toml",
                this.display()
            )
            .into()
        })
}

fn build_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = vec![];
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if name == "diffpilot" {
            out.push(entry.path());
        } else if name.starts_with("diffpilot-") {
            for child in fs::read_dir(entry.path())? {
                let child = child?;
                if child.file_type()?.is_dir() {
                    out.push(child.path());
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

fn live_build_dirs() -> Result<BTreeSet<PathBuf>> {
    let mut out = BTreeSet::new();
    for line in git(&["worktree", "list", "--porcelain"])? {
        let Some(worktree) = line.strip_prefix("worktree ") else {
            continue;
        };
        if !Path::new(worktree).join("Cargo.toml").is_file() {
            continue;
        }
        let metadata = Command::new("cargo")
            .args(["metadata", "--no-deps", "--format-version", "1", "--offline"])
            .current_dir(worktree)
            .output()?;
        if !metadata.status.success() {
            return Err(format!(
                "cannot find the build dir of worktree {worktree}, so nothing was deleted:\n{}",
                String::from_utf8_lossy(&metadata.stderr).trim()
            )
            .into());
        }
        let meta: Value = serde_json::from_slice(&metadata.stdout)?;
        if let Some(dir) = meta["build_directory"].as_str() {
            out.insert(dir.into());
        }
    }
    Ok(out)
}

fn disk_kb(path: &Path) -> Result<u64> {
    let out = Command::new("du").arg("-sk").arg(path).output()?;
    let text = String::from_utf8(out.stdout)?;
    text.split_whitespace()
        .next()
        .and_then(|kb| kb.parse().ok())
        .ok_or_else(|| format!("du printed no size for {}", path.display()).into())
}

// A build that compiles anything writes into <profile>/deps or <target>/<profile>/deps, so three levels are enough.
fn last_used(path: &Path) -> SystemTime {
    fn newest(path: &Path, depth: u8) -> SystemTime {
        let own = fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if depth == 0 {
            return own;
        }
        fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| newest(&entry.path(), depth - 1))
            .fold(own, SystemTime::max)
    }
    newest(path, 3)
}

fn gb(kb: u64) -> f64 {
    kb as f64 / KB_PER_GB as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(60 * 60);

    fn dir(gb: u64, idle_hours: u32, owner: Owner, now: SystemTime) -> BuildDir {
        BuildDir {
            path: PathBuf::new(),
            kb: gb * KB_PER_GB,
            last_used: now - HOUR * idle_hours,
            owner,
        }
    }

    #[test]
    fn deletes_orphans_even_when_under_the_limit() {
        let now = SystemTime::now();
        let dirs = [dir(5, 0, Owner::None, now), dir(5, 9, Owner::OtherWorktree, now)];
        assert_eq!(
            plan(&dirs, 100 * KB_PER_GB, now),
            [Verdict::DeleteOrphan, Verdict::Keep]
        );
    }

    #[test]
    fn deletes_the_least_recently_used_dirs_until_the_rest_fits() {
        let now = SystemTime::now();
        let dirs = [
            dir(10, 3, Owner::OtherWorktree, now),
            dir(10, 30, Owner::OtherWorktree, now),
            dir(10, 9, Owner::OtherWorktree, now),
            dir(10, 2, Owner::ThisWorktree, now),
        ];
        assert_eq!(
            plan(&dirs, 25 * KB_PER_GB, now),
            [
                Verdict::Keep,
                Verdict::DeleteOverLimit,
                Verdict::DeleteOverLimit,
                Verdict::Keep
            ]
        );
    }

    #[test]
    fn keeps_this_worktree_and_dirs_written_in_the_last_hour() {
        let now = SystemTime::now();
        let dirs = [
            dir(10, 99, Owner::ThisWorktree, now),
            dir(10, 0, Owner::OtherWorktree, now),
        ];
        assert_eq!(plan(&dirs, KB_PER_GB, now), [Verdict::Keep, Verdict::Keep]);
    }
}
