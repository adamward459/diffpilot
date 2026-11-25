//! What the Settings window needs to know about the Claude Code CLI on this Mac: whether it is there,
//! whether it is logged in, and whether the diffpilot MCP server is installed for terminal agents.
//!
//! The login check follows diffity's `agents/detect.rs` (MIT, see THIRD_PARTY_NOTICES): `claude auth status
//! --json` and its `loggedIn` field. Every function blocks on a child process, so call them from a background
//! executor.

use crate::claude::{claude_binary, claude_command};
use crate::paths::sibling_binary;
use crate::policy::MCP_SERVER;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const POLL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    LoggedIn,
    LoggedOut,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeStatus {
    NotFound,
    Found { path: PathBuf, login: Login },
}

impl ClaudeStatus {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::NotFound => None,
            Self::Found { path, .. } => Some(path),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpState {
    Connected,
    InstalledNotConnected(String),
    NotInstalled,
    Unknown(String),
}

pub fn resolve_binary(custom: Option<&Path>) -> Option<PathBuf> {
    match custom {
        Some(path) => Some(path.to_path_buf()),
        None => claude_binary(),
    }
}

pub fn mcp_server_binary() -> Option<PathBuf> {
    sibling_binary("diffpilot-mcp")
}

/// `loggedIn` from the JSON `claude auth status --json` prints. `None` when the text is not that JSON.
pub fn parse_auth_status(stdout: &str) -> Option<bool> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    value.get("loggedIn")?.as_bool()
}

/// The login of a found binary. A logged-out CLI exits with 1 and still prints the JSON, so the exit code is
/// not read.
pub fn check_login(binary: &Path) -> Login {
    let output = match probe(binary, &["auth", "status", "--json"]) {
        Ok(output) => output,
        Err(message) => return Login::Unknown(message),
    };
    match parse_auth_status(&output.stdout) {
        Some(true) => Login::LoggedIn,
        Some(false) => Login::LoggedOut,
        None => Login::Unknown(first_line(&output.text()).unwrap_or_else(|| {
            "This version of Claude Code did not answer `claude auth status --json`. Update it, then try again.".into()
        })),
    }
}

pub fn status(custom: Option<&Path>) -> ClaudeStatus {
    match resolve_binary(custom) {
        Some(path) if path.is_file() => {
            let login = check_login(&path);
            ClaudeStatus::Found { path, login }
        }
        _ => ClaudeStatus::NotFound,
    }
}

/// Runs the file with `--version`. A script that is not Claude Code can pass this, so the answer must also
/// name Claude.
pub fn validate_binary(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!("{} is not a file.", path.display()));
    }
    let output = probe(path, &["--version"])?;
    let text = output.stdout.trim().to_string();
    if !output.ok || !text.to_lowercase().contains("claude") {
        return Err(format!("{} did not answer like Claude Code.", path.display()));
    }
    Ok(text)
}

pub fn parse_mcp_get(ok: bool, text: &str) -> McpState {
    let status = text.lines().find_map(|line| line.trim().strip_prefix("Status:"));
    match status {
        Some(status) if status.contains("Connected") => McpState::Connected,
        Some(status) => {
            let issue = text.lines().find_map(|line| line.trim().strip_prefix("Issue:"));
            let detail = issue
                .unwrap_or(status)
                .trim()
                .trim_start_matches(['✘', '✗', '✓'])
                .trim();
            McpState::InstalledNotConnected(detail.to_string())
        }
        None if !ok && text.contains("No MCP server named") => McpState::NotInstalled,
        None => McpState::Unknown(first_line(text).unwrap_or_else(|| "`claude mcp get` printed nothing.".into())),
    }
}

pub fn mcp_state(binary: &Path) -> McpState {
    match probe(binary, &["mcp", "get", MCP_SERVER]) {
        Ok(output) => parse_mcp_get(output.ok, &output.text()),
        Err(message) => McpState::Unknown(message),
    }
}

/// The arguments of `claude mcp add` that register `server` for every project of this user. `--terminal` makes
/// `diffpilot-mcp` find the app's socket on its own.
pub fn mcp_add_args(server: &Path) -> Vec<String> {
    let mut args: Vec<String> = ["mcp", "add", "--scope", "user", MCP_SERVER, "--"]
        .map(String::from)
        .into();
    args.push(server.to_string_lossy().into_owned());
    args.push("--terminal".into());
    args
}

pub fn mcp_remove_args() -> Vec<String> {
    ["mcp", "remove", "--scope", "user", MCP_SERVER]
        .map(String::from)
        .into()
}

/// Removes an older entry, adds the current one, and returns the state `claude mcp get` reports afterwards.
pub fn install_mcp(binary: &Path, server: &Path) -> Result<McpState, String> {
    let _ = run(binary, &mcp_remove_args());
    let added = run(binary, &mcp_add_args(server))?;
    if !added.ok {
        return Err(first_line(&added.text()).unwrap_or_else(|| "`claude mcp add` failed.".into()));
    }
    Ok(mcp_state(binary))
}

struct Probed {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Probed {
    fn text(&self) -> String {
        format!("{}\n{}", self.stdout.trim(), self.stderr.trim())
    }
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(String::from)
}

fn probe(binary: &Path, args: &[&str]) -> Result<Probed, String> {
    let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    run(binary, &args)
}

fn run(binary: &Path, args: &[String]) -> Result<Probed, String> {
    let home = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    let mut command = claude_command(binary, &home);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not run {}: {error}.", binary.display()))?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} did not answer in time.", binary.display()));
            }
            Err(error) => return Err(format!("Could not wait for {}: {error}.", binary.display())),
        }
    };
    Ok(Probed {
        ok: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            text = String::from_utf8_lossy(&bytes).into_owned();
        }
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake(dir: &Path, name: &str, script: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    const LOGGED_IN: &str = r#"{"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty"}"#;
    const LOGGED_OUT: &str = r#"{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}"#;

    #[test]
    fn parses_the_logged_in_flag() {
        assert_eq!(parse_auth_status(LOGGED_IN), Some(true));
        assert_eq!(parse_auth_status(&format!("{LOGGED_OUT}\n")), Some(false));
        assert_eq!(parse_auth_status("Not logged in"), None);
        assert_eq!(parse_auth_status(r#"{"loggedIn": "yes"}"#), None);
        assert_eq!(parse_auth_status(r#"{"authMethod": "none"}"#), None);
        assert_eq!(parse_auth_status(""), None);
    }

    #[test]
    fn reads_each_login_state_from_the_command() {
        let dir = tempfile::tempdir().unwrap();
        let logged_in = fake(dir.path(), "in", &format!("echo '{LOGGED_IN}'"));
        let logged_out = fake(dir.path(), "out", &format!("echo '{LOGGED_OUT}'; exit 1"));
        let old = fake(dir.path(), "old", "echo 'error: unknown command auth' >&2; exit 1");
        assert_eq!(check_login(&logged_in), Login::LoggedIn);
        assert_eq!(check_login(&logged_out), Login::LoggedOut);
        assert_eq!(check_login(&old), Login::Unknown("error: unknown command auth".into()));
    }

    #[test]
    fn a_missing_binary_is_not_found_and_a_found_one_reports_its_login() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(status(Some(&dir.path().join("nope"))), ClaudeStatus::NotFound);
        let claude = fake(dir.path(), "claude", &format!("echo '{LOGGED_IN}'"));
        assert_eq!(
            status(Some(&claude)),
            ClaudeStatus::Found {
                path: claude.clone(),
                login: Login::LoggedIn
            }
        );
        assert_eq!(status(Some(&claude)).path(), Some(claude.as_path()));
    }

    #[test]
    fn a_command_that_hangs_gives_up() {
        let dir = tempfile::tempdir().unwrap();
        let hangs = fake(dir.path(), "hang", "sleep 30");
        let started = Instant::now();
        let result = run(&hangs, &[]);
        assert!(result.err().unwrap().contains("did not answer in time"));
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn validating_runs_the_file_and_wants_claude_in_the_answer() {
        let dir = tempfile::tempdir().unwrap();
        let claude = fake(dir.path(), "claude", "echo '2.1.0 (Claude Code)'");
        let other = fake(dir.path(), "other", "echo 'GNU bash 5'");
        let broken = fake(dir.path(), "broken", "echo 'Claude Code'; exit 3");
        assert_eq!(validate_binary(&claude).unwrap(), "2.1.0 (Claude Code)");
        assert!(
            validate_binary(&other)
                .unwrap_err()
                .contains("did not answer like Claude Code")
        );
        assert!(validate_binary(&broken).is_err());
        assert!(
            validate_binary(&dir.path().join("missing"))
                .unwrap_err()
                .contains("is not a file")
        );
        assert!(validate_binary(dir.path()).is_err());
    }

    #[test]
    fn reads_the_mcp_state_from_claude_mcp_get() {
        let connected =
            "diffpilot:\n  Scope: User config (available in all your projects)\n  Status: ✓ Connected\n  Type: stdio\n";
        let failed = "diffpilot:\n  Status: ✘ Failed to connect\n  Issue: CONNECTION_CLOSED: Connection closed\n";
        let failed_no_issue = "diffpilot:\n  Status: ✘ Failed to connect\n";
        let missing = "No MCP server named \"diffpilot\". Configured servers: a, b";
        assert_eq!(parse_mcp_get(true, connected), McpState::Connected);
        assert_eq!(
            parse_mcp_get(true, failed),
            McpState::InstalledNotConnected("CONNECTION_CLOSED: Connection closed".into())
        );
        assert_eq!(
            parse_mcp_get(true, failed_no_issue),
            McpState::InstalledNotConnected("Failed to connect".into())
        );
        assert_eq!(parse_mcp_get(false, missing), McpState::NotInstalled);
        assert_eq!(
            parse_mcp_get(false, "Segmentation fault"),
            McpState::Unknown("Segmentation fault".into())
        );
        assert_eq!(
            parse_mcp_get(false, ""),
            McpState::Unknown("`claude mcp get` printed nothing.".into())
        );
    }

    #[test]
    fn the_add_arguments_register_the_server_for_the_user_with_terminal_mode() {
        assert_eq!(
            mcp_add_args(Path::new("/Applications/diffpilot.app/Contents/MacOS/diffpilot-mcp")),
            [
                "mcp",
                "add",
                "--scope",
                "user",
                "diffpilot",
                "--",
                "/Applications/diffpilot.app/Contents/MacOS/diffpilot-mcp",
                "--terminal"
            ]
        );
        assert_eq!(mcp_remove_args(), ["mcp", "remove", "--scope", "user", "diffpilot"]);
    }

    #[test]
    fn installing_removes_the_old_entry_then_adds_and_reads_the_state_back() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls");
        let state = dir.path().join("installed");
        let script = format!(
            r#"echo "$@" >> '{log}'
case "$2" in
  remove) rm -f '{state}'; exit 0 ;;
  add) touch '{state}'; exit 0 ;;
  get) if [ -f '{state}' ]; then echo '  Status: ✓ Connected'; exit 0; else echo 'No MCP server named "diffpilot".'; exit 1; fi ;;
esac"#,
            log = log.display(),
            state = state.display()
        );
        let claude = fake(dir.path(), "claude", &script);
        assert_eq!(mcp_state(&claude), McpState::NotInstalled);
        let server = Path::new("/bin/diffpilot-mcp");
        assert_eq!(install_mcp(&claude, server), Ok(McpState::Connected));
        assert_eq!(install_mcp(&claude, server), Ok(McpState::Connected));
        let calls = std::fs::read_to_string(&log).unwrap();
        let adds: Vec<&str> = calls.lines().filter(|line| line.starts_with("mcp add")).collect();
        assert_eq!(
            adds,
            ["mcp add --scope user diffpilot -- /bin/diffpilot-mcp --terminal"; 2]
        );
        assert_eq!(calls.lines().filter(|line| line.starts_with("mcp remove")).count(), 2);
    }

    #[test]
    fn a_failed_add_reports_its_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let claude = fake(
            dir.path(),
            "claude",
            r#"if [ "$2" = add ]; then echo 'cannot write config' >&2; exit 1; fi"#,
        );
        assert_eq!(
            install_mcp(&claude, Path::new("/bin/diffpilot-mcp")),
            Err("cannot write config".into())
        );
    }
}
