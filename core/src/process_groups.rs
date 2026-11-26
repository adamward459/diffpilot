//! Each claude CLI runs in its own process group, so nothing stops it when diffpilot exits.
//! This module keeps the list of live groups, stops them on quit, and stops the ones a crashed diffpilot left behind.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(20);
const QUIT_GRACE: Duration = Duration::from_millis(50);
const ORPHAN_GRACE: Duration = Duration::from_secs(2);

/// A start time comes with the pid, so a pid that the OS gave to a new process never matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Process {
    pid: i32,
    started_us: u64,
}

impl Process {
    /// None when no process has this pid, or when it does not lead its own process group.
    fn leader(pid: i32) -> Option<Self> {
        bsd_info(pid).and_then(|(process, group)| (group == pid).then_some(process))
    }

    fn current() -> Option<Self> {
        bsd_info(std::process::id() as i32).map(|(process, _)| process)
    }

    fn is_running(&self) -> bool {
        bsd_info(self.pid).is_some_and(|(process, _)| process == *self)
    }

    fn is_alive_leader(&self) -> bool {
        Self::leader(self.pid) == Some(*self)
    }

    fn signal_group(&self, signal: libc::c_int) {
        // SAFETY: killpg only sends a signal.
        unsafe { libc::killpg(self.pid, signal) };
    }
}

#[cfg(target_os = "macos")]
fn bsd_info(pid: i32) -> Option<(Process, i32)> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: proc_pidinfo writes at most `size` bytes into `info`.
    let written = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    let process = Process {
        pid,
        started_us: info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec,
    };
    (written == size).then_some((process, info.pbi_pgid as i32))
}

#[cfg(not(target_os = "macos"))]
fn bsd_info(_pid: i32) -> Option<(Process, i32)> {
    None
}

#[derive(Serialize, Deserialize)]
struct Record {
    owner: Process,
    groups: Vec<Process>,
}

struct Live {
    groups: Vec<Process>,
    file: Option<PathBuf>,
}

static LIVE: Mutex<Live> = Mutex::new(Live {
    groups: Vec::new(),
    file: None,
});

impl Live {
    fn save(&self) {
        let Some(file) = &self.file else { return };
        let (Some(owner), false) = (Process::current(), self.groups.is_empty()) else {
            let _ = std::fs::remove_file(file);
            return;
        };
        let record = Record {
            owner,
            groups: self.groups.clone(),
        };
        let partial = file.with_extension("tmp");
        if let Ok(json) = serde_json::to_vec(&record)
            && std::fs::write(&partial, json).is_ok()
        {
            let _ = std::fs::rename(&partial, file);
        }
    }
}

/// Stays in the live list until it is dropped, which must happen after the child is reaped.
pub(crate) struct Tracked(Option<Process>);

impl Tracked {
    pub(crate) fn start(pid: u32) -> Self {
        let process = i32::try_from(pid).ok().and_then(Process::leader);
        if let Some(process) = process {
            let mut live = LIVE.lock().unwrap();
            live.groups.push(process);
            live.save();
        }
        Self(process)
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        let Some(process) = self.0 else { return };
        let mut live = LIVE.lock().unwrap();
        live.groups.retain(|group| *group != process);
        live.save();
    }
}

/// Records this app's live groups in `dir/<pid>.json`, so the next launch can stop them after a crash.
pub fn record_in(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let mut live = LIVE.lock().unwrap();
    live.file = Some(dir.join(format!("{}.json", std::process::id())));
    live.save();
}

/// Stops every live group: SIGTERM, a short wait, then SIGKILL. It blocks for about 50 ms, so it fits in the 100 ms that GPUI gives quit handlers.
pub fn stop_all() {
    let mut live = LIVE.lock().unwrap();
    let groups = std::mem::take(&mut live.groups);
    groups.iter().for_each(|group| group.signal_group(libc::SIGTERM));
    let started = Instant::now();
    while started.elapsed() < QUIT_GRACE && groups.iter().any(Process::is_alive_leader) {
        std::thread::sleep(POLL);
    }
    groups.iter().for_each(|group| group.signal_group(libc::SIGKILL));
    live.save();
}

/// Stops the groups in the records of diffpilot instances that are no longer running, then deletes those records.
/// It blocks for up to 2 seconds, so call it off the main thread.
pub fn reap_orphans(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut orphans = Vec::new();
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Some(record) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok())
        else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        if record.owner.is_running() {
            continue;
        }
        orphans.extend(record.groups.into_iter().filter(Process::is_alive_leader));
        let _ = std::fs::remove_file(&path);
    }
    orphans.iter().for_each(|group| group.signal_group(libc::SIGTERM));
    let started = Instant::now();
    while started.elapsed() < ORPHAN_GRACE && orphans.iter().any(Process::is_alive_leader) {
        std::thread::sleep(POLL);
    }
    orphans
        .iter()
        .filter(|group| group.is_alive_leader())
        .for_each(|group| group.signal_group(libc::SIGKILL));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    fn sleeper() -> (Child, Process) {
        let child = Command::new("sleep").arg("300").process_group(0).spawn().unwrap();
        let process = Process::leader(child.id() as i32).unwrap();
        (child, process)
    }

    fn dead_owner() -> Process {
        let (mut owner, process) = sleeper();
        owner.kill().unwrap();
        owner.wait().unwrap();
        process
    }

    fn write_record(dir: &Path, owner: Process, groups: Vec<Process>) -> PathBuf {
        let path = dir.join(format!("{}.json", owner.pid));
        std::fs::write(&path, serde_json::to_vec(&Record { owner, groups }).unwrap()).unwrap();
        path
    }

    fn exits_within(child: &mut Child, limit: Duration) -> bool {
        let started = Instant::now();
        while started.elapsed() < limit {
            if child.try_wait().unwrap().is_some() {
                return true;
            }
            std::thread::sleep(POLL);
        }
        false
    }

    #[test]
    fn reaping_stops_the_groups_of_an_owner_that_exited() {
        let dir = tempfile::tempdir().unwrap();
        let (mut orphan, group) = sleeper();
        let record = write_record(dir.path(), dead_owner(), vec![group]);
        reap_orphans(dir.path());
        assert!(exits_within(&mut orphan, Duration::from_secs(1)));
        assert!(!record.exists());
    }

    #[test]
    fn reaping_leaves_the_groups_of_a_running_owner() {
        let dir = tempfile::tempdir().unwrap();
        let (mut child, group) = sleeper();
        let record = write_record(dir.path(), Process::current().unwrap(), vec![group]);
        reap_orphans(dir.path());
        let survived = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(survived);
        assert!(record.exists());
    }

    #[test]
    fn reaping_skips_a_pid_that_now_belongs_to_another_process() {
        let dir = tempfile::tempdir().unwrap();
        let (mut child, group) = sleeper();
        let reused = Process {
            started_us: group.started_us + 1,
            ..group
        };
        write_record(dir.path(), dead_owner(), vec![reused]);
        reap_orphans(dir.path());
        let survived = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(survived);
    }
}
