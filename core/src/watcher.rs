use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const LATENCY: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepoChange {
    pub head_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub is_relevant: bool,
    pub head_changed: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot watch the repository: {0}")]
pub struct WatchError(#[from] notify::Error);

/// Stops watching when dropped, and then closes the change channel.
pub struct WatchGuard {
    _watcher: RecommendedWatcher,
}

pub fn watch(repo: &Path) -> Result<(WatchGuard, async_channel::Receiver<RepoChange>), WatchError> {
    watch_with_latency(repo, LATENCY)
}

pub fn watch_with_latency(
    repo: &Path,
    latency: Duration,
) -> Result<(WatchGuard, async_channel::Receiver<RepoChange>), WatchError> {
    let root = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    let (events, batches) = mpsc::channel::<PathBuf>();
    let (sender, receiver) = async_channel::unbounded();

    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let Ok(event) = event else { return };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        for path in event.paths {
            let _ = events.send(path);
        }
    })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;

    let thread_root = root.to_string_lossy().into_owned();
    std::thread::spawn(move || {
        while let Ok(first) = batches.recv() {
            let mut paths = vec![first];
            let deadline = Instant::now() + latency;
            let mut open = true;
            while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                match batches.recv_timeout(left) {
                    Ok(path) => paths.push(path),
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        open = false;
                        break;
                    }
                }
            }
            let names: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            let summary = summarize(&names, &thread_root);
            let change = RepoChange {
                head_changed: summary.head_changed,
            };
            if summary.is_relevant && sender.send_blocking(change).is_err() {
                break;
            }
            if !open {
                break;
            }
        }
    });
    Ok((WatchGuard { _watcher: watcher }, receiver))
}

pub fn summarize<S: AsRef<str>>(paths: &[S], root: &str) -> Summary {
    let mut summary = Summary::default();
    for path in paths {
        let Some(relative) = relative_path(path.as_ref(), root) else {
            continue;
        };
        if !is_relevant_path(relative) {
            continue;
        }
        summary.is_relevant = true;
        if relative == ".git/HEAD" {
            summary.head_changed = true;
        }
    }
    summary
}

pub fn is_relevant_path(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').filter(|part| !part.is_empty()).collect();
    let Some(first) = parts.first() else { return true };
    if *first == ".git" {
        let Some(second) = parts.get(1) else { return false };
        return match *second {
            "HEAD" | "index" | "packed-refs" => parts.len() == 2,
            "refs" => parts.len() > 2,
            _ => false,
        };
    }
    !parts.contains(&".git")
}

fn relative_path<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    if path == root {
        return Some("");
    }
    let prefix = if root.ends_with('/') {
        root.to_string()
    } else {
        format!("{root}/")
    };
    path.strip_prefix(&prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::init_repo;

    #[test]
    fn keeps_working_tree_and_git_state_paths() {
        for path in [
            "src/a.ts",
            "a.ts",
            ".gitignore",
            ".github/workflows/ci.yml",
            ".git/HEAD",
            ".git/index",
            ".git/packed-refs",
            ".git/refs/heads/main",
            ".git/refs/remotes/origin/main",
        ] {
            assert!(is_relevant_path(path), "{path}");
        }
    }

    #[test]
    fn drops_git_internals() {
        for path in [
            ".git",
            ".git/",
            ".git/objects/ab/cdef",
            ".git/index.lock",
            ".git/HEAD.lock",
            ".git/logs/HEAD",
            ".git/refs",
            ".git/FETCH_HEAD",
            ".git/COMMIT_EDITMSG",
            ".git/config",
            "vendor/lib/.git/HEAD",
            "vendor/lib/.git/index",
        ] {
            assert!(!is_relevant_path(path), "{path}");
        }
    }

    #[test]
    fn reports_head_changes_and_ignores_paths_outside_the_root() {
        let summary = |paths: &[&str]| {
            let s = summarize(paths, "/r");
            (s.is_relevant, s.head_changed)
        };
        assert_eq!(summary(&["/r/.git/HEAD"]), (true, true));
        assert_eq!(summary(&["/r/src/a.ts", "/r/.git/objects/x"]), (true, false));
        assert_eq!(summary(&["/r/.git/objects/x", "/other/a.ts"]), (false, false));
    }

    fn first_change(repo: &Path, mut touch: impl FnMut() + Send + 'static) -> Option<RepoChange> {
        // FSEvents can replay the events of the `git init` that just ran.
        std::thread::sleep(Duration::from_millis(1000));
        let (_guard, changes) = watch_with_latency(repo, Duration::from_millis(100)).unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let toucher = std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(250));
                touch();
            }
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut found = None;
        while found.is_none() && Instant::now() < deadline {
            found = changes.try_recv().ok();
            std::thread::sleep(Duration::from_millis(25));
        }
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        toucher.join().unwrap();
        found
    }

    #[test]
    fn reports_a_working_tree_edit_without_a_head_change() {
        let repo = init_repo();
        let file = repo.path().join("a.txt");
        let mut count = 0;
        let change = first_change(repo.path(), move || {
            count += 1;
            let _ = std::fs::write(&file, format!("x {count}"));
        });
        assert_eq!(change, Some(RepoChange { head_changed: false }));
    }

    #[test]
    fn reports_a_head_change() {
        let repo = init_repo();
        let head = repo.path().join(".git/HEAD");
        let mut count = 0;
        let change = first_change(repo.path(), move || {
            count += 1;
            let _ = std::fs::write(&head, format!("ref: refs/heads/other-{count}\n"));
        });
        assert_eq!(change, Some(RepoChange { head_changed: true }));
    }

    #[test]
    fn dropping_the_guard_closes_the_channel() {
        let repo = init_repo();
        let (guard, changes) = watch(repo.path()).unwrap();
        drop(guard);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !changes.is_closed() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(changes.is_closed());
    }
}
