use diffpilot_core::guard::{HookDecision, decide_hook};
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitCode};

fn alias(repo: &Path, name: &str) -> Option<String> {
    let git = diffpilot_core::git::binary().unwrap_or_else(|| "git".into());
    let output = Command::new(git)
        .arg("-C")
        .arg(repo)
        .args(["config", "--get", &format!("alias.{name}")])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !text.is_empty()).then_some(text)
}

/// The PreToolUse hook command of a run. Exit 2 blocks the Bash call and shows stderr to Claude.
fn main() -> ExitCode {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("{}", diffpilot_core::guard::Violation::TooNested.message());
        return ExitCode::from(2);
    }
    let cwd = std::env::current_dir().ok();
    match decide_hook(&input, cwd.as_deref(), &alias) {
        HookDecision::Allow => ExitCode::SUCCESS,
        HookDecision::Block(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
