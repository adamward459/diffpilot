//! What a run may do: the tools it gets, the permission setting, and the settings file that installs the
//! PreToolUse hook.
//!
//! The per-kind tool lists follow diffity's `agents/policy.rs` (MIT, see THIRD_PARTY_NOTICES). diffity has no
//! push filter, only prompt text. Every diffpilot run adds the deny list and the hook.

use crate::run::RunKind;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;

/// Every run passes these, whatever the permission setting says.
pub const DENIED_TOOLS: [&str; 3] = ["Bash(git push:*)", "Bash(git commit:*)", "Bash(gh:*)"];

pub const MCP_SERVER: &str = "diffpilot";
pub const APPROVE_TOOL: &str = "approve";

pub fn mcp_tool(name: &str) -> String {
    format!("mcp__{MCP_SERVER}__{name}")
}

const READ_ONLY_GIT: [&str; 4] = [
    "Bash(git diff:*)",
    "Bash(git log:*)",
    "Bash(git show:*)",
    "Bash(git status:*)",
];

/// Whether Claude may edit files without asking. The setting only applies to runs that can edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditPermission {
    #[default]
    DontAsk,
    AskOnce,
    AskEach,
}

impl EditPermission {
    pub const ALL: [EditPermission; 3] = [Self::DontAsk, Self::AskOnce, Self::AskEach];

    pub fn title(self) -> &'static str {
        match self {
            Self::DontAsk => "Don't ask",
            Self::AskOnce => "Ask once per run",
            Self::AskEach => "Ask for every edit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("Asking before an edit needs the diffpilot MCP server, which is not available.")]
    PromptNeedsMcp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPolicy {
    pub tools: Vec<String>,
    pub allowed: Vec<String>,
    pub disallowed: Vec<String>,
    pub permission_mode: Option<&'static str>,
    pub permission_prompt_tool: Option<String>,
}

impl ToolPolicy {
    /// `edits` is how the run may edit files, None when it may not. `has_mcp` says whether the run gets the diffpilot
    /// MCP server.
    pub fn new(kind: RunKind, edits: Option<EditPermission>, has_mcp: bool) -> Result<Self, PolicyError> {
        let mut tools = vec!["Read", "Grep", "Glob", "TodoWrite", "Bash"];
        let mut allowed: Vec<String> = vec!["Read".into(), "Grep".into(), "Glob".into(), "TodoWrite".into()];
        if has_mcp {
            allowed.extend(kind.activity().mcp_tools().iter().map(|name| mcp_tool(name)));
        }
        let mut permission_mode = None;
        let mut permission_prompt_tool = None;
        if let Some(edit) = edits.filter(|_| kind.activity().edits()) {
            tools.extend(["Edit", "Write"]);
            match edit {
                EditPermission::DontAsk => {
                    allowed.extend(["Edit", "Write", "Bash"].map(String::from));
                    permission_mode = Some("acceptEdits");
                }
                EditPermission::AskOnce | EditPermission::AskEach => {
                    if !has_mcp {
                        return Err(PolicyError::PromptNeedsMcp);
                    }
                    allowed.extend(READ_ONLY_GIT.map(String::from));
                    permission_prompt_tool = Some(mcp_tool(APPROVE_TOOL));
                }
            }
        } else {
            allowed.extend(READ_ONLY_GIT.map(String::from));
        }
        Ok(Self {
            tools: tools.into_iter().map(String::from).collect(),
            allowed,
            disallowed: DENIED_TOOLS.map(String::from).into(),
            permission_mode,
            permission_prompt_tool,
        })
    }

    pub fn arguments(&self) -> Vec<String> {
        let mut args = vec!["--tools".to_string(), self.tools.join(",")];
        args.push("--allowedTools".into());
        args.extend(self.allowed.iter().cloned());
        args.push("--disallowedTools".into());
        args.extend(self.disallowed.iter().cloned());
        if let Some(mode) = self.permission_mode {
            args.extend(["--permission-mode".to_string(), mode.to_string()]);
        }
        if let Some(tool) = &self.permission_prompt_tool {
            args.extend(["--permission-prompt-tool".to_string(), tool.clone()]);
        }
        args
    }
}

/// The file passed with `--mcp-config`: `diffpilot-mcp` as a stdio server that forwards to the app over `socket`.
pub fn mcp_config(binary: &Path, socket: &Path, token: &str) -> Value {
    json!({
        "mcpServers": {
            MCP_SERVER: {
                "type": "stdio",
                "command": binary.to_string_lossy(),
                "args": ["--socket", socket.to_string_lossy(), "--token", token],
            }
        }
    })
}

pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The settings file passed with `--settings`. It is loaded after the user's and the project's, so a project
/// cannot turn the hook off.
pub fn hook_settings(guard: &Path) -> Value {
    json!({
        "disableAllHooks": false,
        "hooks": {
            "PreToolUse": [{
                "matcher": "Bash",
                "hooks": [{ "type": "command", "command": shell_quote(&guard.to_string_lossy()) }]
            }]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(kind: RunKind, edit: EditPermission, has_mcp: bool) -> ToolPolicy {
        ToolPolicy::new(kind, kind.activity().edits().then_some(edit), has_mcp).unwrap()
    }

    fn after<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
        let at = args.iter().position(|arg| arg == flag).unwrap();
        args[at + 1..]
            .iter()
            .take_while(|arg| !arg.starts_with("--"))
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn every_run_carries_the_push_deny_list() {
        for kind in RunKind::ALL {
            for edit in EditPermission::ALL {
                let Ok(policy) = ToolPolicy::new(kind, kind.activity().edits().then_some(edit), true) else {
                    continue;
                };
                let args = policy.arguments();
                assert_eq!(
                    after(&args, "--disallowedTools"),
                    ["Bash(git push:*)", "Bash(git commit:*)", "Bash(gh:*)"],
                    "{kind:?} {edit:?}"
                );
            }
        }
    }

    #[test]
    fn a_review_cannot_edit_and_may_only_read_git() {
        let args = policy(RunKind::Review, EditPermission::DontAsk, true).arguments();
        assert_eq!(after(&args, "--tools"), ["Read,Grep,Glob,TodoWrite,Bash"]);
        let allowed = after(&args, "--allowedTools");
        assert!(!allowed.contains(&"Edit") && !allowed.contains(&"Write") && !allowed.contains(&"Bash"));
        assert!(allowed.contains(&"Bash(git diff:*)"));
        assert!(allowed.contains(&"mcp__diffpilot__add_comment"));
        assert!(!args.contains(&"--permission-mode".to_string()));
    }

    #[test]
    fn a_guide_run_reads_and_writes_only_the_guide() {
        for edit in EditPermission::ALL {
            let args = policy(RunKind::Guide, edit, true).arguments();
            assert!(!args.contains(&"--permission-mode".to_string()), "{edit:?}");
            assert!(!args.contains(&"--permission-prompt-tool".to_string()), "{edit:?}");
            let allowed = after(&args, "--allowedTools");
            assert!(!allowed.contains(&"Edit") && !allowed.contains(&"Write") && !allowed.contains(&"Bash"));
            assert!(allowed.contains(&"Grep") && allowed.contains(&"Bash(git log:*)"));
            for tool in ["set_overview", "add_chapter", "add_note", "add_diagram", "finish_guide"] {
                assert!(allowed.contains(&format!("mcp__diffpilot__{tool}").as_str()), "{tool}");
            }
            assert!(!allowed.contains(&"mcp__diffpilot__add_comment"));
            assert_eq!(
                after(&args, "--disallowedTools"),
                ["Bash(git push:*)", "Bash(git commit:*)", "Bash(gh:*)"]
            );
        }
    }

    #[test]
    fn dont_ask_lets_a_fix_edit_without_prompts() {
        let args = policy(RunKind::FixComments, EditPermission::DontAsk, true).arguments();
        assert_eq!(after(&args, "--tools"), ["Read,Grep,Glob,TodoWrite,Bash,Edit,Write"]);
        let allowed = after(&args, "--allowedTools");
        assert!(allowed.contains(&"Edit") && allowed.contains(&"Write") && allowed.contains(&"Bash"));
        assert!(allowed.contains(&"mcp__diffpilot__resolve"));
        assert_eq!(after(&args, "--permission-mode"), ["acceptEdits"]);
        assert!(!args.contains(&"--permission-prompt-tool".to_string()));
    }

    #[test]
    fn a_read_only_reply_can_answer_and_resolve_but_not_edit() {
        let args = ToolPolicy::new(RunKind::Reply, None, true).unwrap().arguments();
        assert_eq!(after(&args, "--tools"), ["Read,Grep,Glob,TodoWrite,Bash"]);
        let allowed = after(&args, "--allowedTools");
        assert!(!allowed.contains(&"Edit") && !allowed.contains(&"Bash"));
        assert!(allowed.contains(&"mcp__diffpilot__reply") && allowed.contains(&"mcp__diffpilot__resolve"));
        assert!(!args.contains(&"--permission-mode".to_string()));
        assert!(!args.contains(&"--permission-prompt-tool".to_string()));
    }

    #[test]
    fn the_ask_settings_route_edits_to_the_prompt_tool() {
        for edit in [EditPermission::AskOnce, EditPermission::AskEach] {
            let args = policy(RunKind::MakeChanges, edit, true).arguments();
            assert_eq!(after(&args, "--permission-prompt-tool"), ["mcp__diffpilot__approve"]);
            let allowed = after(&args, "--allowedTools");
            assert!(!allowed.contains(&"Edit") && !allowed.contains(&"Bash"));
            assert!(!args.contains(&"--permission-mode".to_string()));
        }
    }

    #[test]
    fn asking_needs_the_mcp_server() {
        assert_eq!(
            ToolPolicy::new(RunKind::FixComments, Some(EditPermission::AskOnce), false),
            Err(PolicyError::PromptNeedsMcp)
        );
        assert!(ToolPolicy::new(RunKind::Review, Some(EditPermission::AskEach), false).is_ok());
        assert!(ToolPolicy::new(RunKind::FixComments, Some(EditPermission::DontAsk), false).is_ok());
        assert!(
            ToolPolicy::new(RunKind::Reply, None, false).is_ok(),
            "a read-only reply asks nothing"
        );
    }

    #[test]
    fn the_deny_list_never_appears_in_the_allow_list() {
        for kind in RunKind::ALL {
            for edit in EditPermission::ALL {
                let policy = ToolPolicy::new(kind, kind.activity().edits().then_some(edit), true).unwrap();
                assert!(policy.allowed.iter().all(|tool| !DENIED_TOOLS.contains(&tool.as_str())));
            }
        }
    }

    #[test]
    fn the_permission_setting_serializes_in_camel_case() {
        assert_eq!(serde_json::to_string(&EditPermission::AskOnce).unwrap(), "\"askOnce\"");
        assert_eq!(
            serde_json::from_str::<EditPermission>("\"dontAsk\"").unwrap(),
            EditPermission::DontAsk
        );
        assert_eq!(EditPermission::default(), EditPermission::DontAsk);
    }

    #[test]
    fn the_hook_settings_run_the_guard_on_every_bash_call_and_survive_odd_paths() {
        let settings = hook_settings(Path::new("/Users/me/My App/it's/diffpilot-guard"));
        let hook = &settings["hooks"]["PreToolUse"][0];
        assert_eq!(hook["matcher"], "Bash");
        assert_eq!(hook["hooks"][0]["type"], "command");
        assert_eq!(
            hook["hooks"][0]["command"],
            "'/Users/me/My App/it'\\''s/diffpilot-guard'"
        );
        assert_eq!(settings["disableAllHooks"], false);
    }
}
