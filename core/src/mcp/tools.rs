//! The tools a run can call: definitions for `tools/list`, and the code that runs them against the store.
//!
//! The tool set and the wording of the descriptions come from diffity's `bin/diffity-mcp/tools.rs` and
//! `agents/tools.rs` (MIT, see THIRD_PARTY_NOTICES). diffpilot adds `title` to `add_comment` and `files` to
//! `resolve`, and has no guide tool yet.

use super::bridge::{Binding, Notice};
use crate::anchor;
use crate::changes;
use crate::diff::{self, FileDiff};
use crate::git;
use crate::policy::{APPROVE_TOOL, EditPermission, mcp_tools};
use crate::review::{FindingMeta, Severity};
use crate::store::{NewComment, NewThread, SessionId, Store};
use crate::threads::{Addressed, CLAUDE_NAME, CommentSource, ReviewThread, Side, ThreadAnchor, ThreadId, ThreadStatus};
use serde::Deserialize;
use serde_json::{Value, json};

pub const GENERAL_PATH: &str = "__general__";
const MAX_DIFF_BYTES: usize = 200 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ToolError(pub String);

impl ToolError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl From<crate::store::StoreError> for ToolError {
    fn from(error: crate::store::StoreError) -> Self {
        Self(error.to_string())
    }
}

impl From<git::GitError> for ToolError {
    fn from(error: git::GitError) -> Self {
        Self(error.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub read_only: bool,
}

const THREAD_ID: &str = "Thread id from list_threads.";

pub fn definitions() -> Vec<ToolDef> {
    let tool = |name, description, input_schema, read_only| ToolDef {
        name,
        description,
        input_schema,
        read_only,
    };
    vec![
        tool(
            "get_diff",
            "Get the unified diff of the review open in diffpilot, preceded by the list of changed files. Line numbers for comments \
             come from the @@ hunk headers: new-side lines for added or context code, old-side lines for removed code. With `summary` \
             it returns only the file list. With `files` it returns only those patches.",
            json!({
                "type": "object",
                "properties": {
                    "summary": { "type": "boolean", "description": "Return only the changed files, with status and +added/-removed." },
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Return the patch of these files only, as the file list names them." }
                }
            }),
            true,
        ),
        tool(
            "list_threads",
            "List the review comment threads of this run, with every comment, author type (user or agent), file path, line range, \
             side, status, severity and `addressedBy`. Diff-level comments have filePath \"__general__\".",
            json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": ["open", "resolved", "dismissed"], "description": "Only return threads with this status. Omit for all." }
                }
            }),
            true,
        ),
        tool(
            "add_comment",
            "Leave an inline review comment on lines of a file in the diff. The file must be part of the diff and the lines must \
             exist on the chosen side. Put the finding in body and its category in severity, not in the body.",
            json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Repository-relative path of a file in the diff." },
                    "startLine": { "type": "integer", "minimum": 0, "description": "First line (1-based) on the chosen side. Use 0 for a file-level comment." },
                    "endLine": { "type": "integer", "minimum": 0, "description": "Last line of the range. Defaults to startLine." },
                    "side": { "type": "string", "enum": ["new", "old"], "description": "\"new\" (default) for added or unchanged code, \"old\" for removed code." },
                    "title": { "type": "string", "description": "One short line naming the problem." },
                    "body": { "type": "string", "description": "Markdown in simple English: the problem and the fix in 1-3 short sentences." },
                    "severity": { "type": "string", "enum": ["must-fix", "suggestion", "nit", "question"], "description": "must-fix: bugs, security, data loss. suggestion: a concrete improvement with a clear reason. nit: minor. question: needs the author's answer." }
                },
                "required": ["file", "startLine", "body", "severity"]
            }),
            false,
        ),
        tool(
            "add_general_comment",
            "Leave a diff-level comment that belongs to no file or line, such as the overall review summary.",
            json!({
                "type": "object",
                "properties": { "body": { "type": "string", "description": "Markdown comment body." } },
                "required": ["body"]
            }),
            false,
        ),
        tool(
            "reply",
            "Reply to an existing thread, to answer it or to ask the reviewer a question.",
            json!({
                "type": "object",
                "properties": {
                    "threadId": { "type": "string", "description": THREAD_ID },
                    "body": { "type": "string", "description": "Markdown reply in simple English, 1-3 short sentences." }
                },
                "required": ["threadId", "body"]
            }),
            false,
        ),
        tool(
            "resolve",
            "Report a thread as done, with a summary of what you changed or the answer to a question. The reviewer's thread stays \
             open, marked as addressed by you, until the reviewer checks it and resolves it.",
            json!({
                "type": "object",
                "properties": {
                    "threadId": { "type": "string", "description": THREAD_ID },
                    "summary": { "type": "string", "description": "One short sentence on what changed, for example \"Fixed: added a null check.\"" },
                    "files": { "type": "array", "items": { "type": "string" }, "description": "Repository-relative paths you changed for this thread." }
                },
                "required": ["threadId"]
            }),
            false,
        ),
        tool(
            "dismiss",
            "Dismiss a thread that should not be acted on, with a reason.",
            json!({
                "type": "object",
                "properties": {
                    "threadId": { "type": "string", "description": THREAD_ID },
                    "reason": { "type": "string", "description": "Why the thread is dismissed." }
                },
                "required": ["threadId"]
            }),
            false,
        ),
        tool(
            APPROVE_TOOL,
            "Used by Claude Code to ask the reviewer before a tool runs. Not for you to call.",
            json!({
                "type": "object",
                "properties": {
                    "tool_name": { "type": "string" },
                    "input": { "type": "object" },
                    "tool_use_id": { "type": "string" }
                },
                "required": ["tool_name", "input"]
            }),
            false,
        ),
    ]
}

/// The names this run may call.
pub fn allowed(binding: &Binding) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = mcp_tools(binding.kind).to_vec();
    if binding.kind.edits() && binding.permission != EditPermission::DontAsk {
        names.push(APPROVE_TOOL);
    }
    names
}

fn parse<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, ToolError> {
    serde_json::from_value(args).map_err(|error| ToolError::new(format!("Bad arguments: {error}")))
}

pub fn thread_id(value: &Value) -> Option<ThreadId> {
    match value {
        Value::String(text) => text.trim().parse().ok().map(ThreadId),
        Value::Number(number) => number.as_u64().and_then(|n| u32::try_from(n).ok()).map(ThreadId),
        _ => None,
    }
}

pub fn call(store: &Store, binding: &Binding, tool: &str, args: Value) -> Result<Value, ToolError> {
    if !allowed(binding).contains(&tool) {
        return Err(ToolError::new(format!(
            "The tool `{tool}` is not available in a {} run.",
            binding.kind.title().to_lowercase()
        )));
    }
    let ctx = Ctx { store, binding };
    match tool {
        "get_diff" => ctx.get_diff(parse(args)?),
        "list_threads" => ctx.list_threads(parse(args)?),
        "add_comment" => ctx.add_comment(parse(args)?),
        "add_general_comment" => ctx.add_general_comment(parse(args)?),
        "reply" => ctx.reply(parse(args)?),
        "resolve" => ctx.resolve(parse(args)?),
        "dismiss" => ctx.dismiss(parse(args)?),
        _ => Err(ToolError::new(format!("Unknown tool `{tool}`."))),
    }
}

struct Ctx<'a> {
    store: &'a Store,
    binding: &'a Binding,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DiffArgs {
    summary: bool,
    files: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListArgs {
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentArgs {
    file: String,
    start_line: u32,
    #[serde(default)]
    end_line: Option<u32>,
    #[serde(default)]
    side: Option<String>,
    #[serde(default)]
    title: Option<String>,
    body: String,
    #[serde(default)]
    severity: Option<String>,
}

#[derive(Deserialize)]
struct GeneralArgs {
    body: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplyArgs {
    thread_id: Value,
    body: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveArgs {
    thread_id: Value,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DismissArgs {
    thread_id: Value,
    #[serde(default)]
    reason: Option<String>,
}

fn claude_comment(body: &str) -> NewComment {
    NewComment {
        author: CLAUDE_NAME.into(),
        source: CommentSource::Claude,
        body: body.trim().to_string(),
        streaming: false,
        pending: false,
    }
}

fn first_sentence(body: &str) -> String {
    let line = body.trim().lines().next().unwrap_or_default();
    let sentence = line.split_inclusive(['.', '?', '!']).next().unwrap_or(line).trim();
    let short: String = sentence.chars().take(80).collect();
    if sentence.chars().count() > 80 {
        format!("{}…", short.trim_end())
    } else {
        short
    }
}

impl Ctx<'_> {
    fn session(&self) -> SessionId {
        self.binding.session
    }

    fn notify(&self, notice: Notice) {
        (self.binding.notify)(notice);
    }

    fn git_diff(&self, files: &[String]) -> Result<String, ToolError> {
        let mut args: Vec<&str> = diff::DIFF_ARGS.to_vec();
        args.extend(self.binding.revisions.iter().map(String::as_str));
        if !files.is_empty() {
            args.push("--");
            args.extend(files.iter().map(String::as_str));
        }
        Ok(git::run(&self.binding.repo, &args)?)
    }

    fn file_list(&self) -> Result<String, ToolError> {
        let run = |mode: &str| -> Result<String, ToolError> {
            let mut args = vec!["diff", "-z", "-M", mode];
            args.extend(self.binding.revisions.iter().map(String::as_str));
            Ok(git::run(&self.binding.repo, &args)?)
        };
        let files = changes::parse(&run("--name-status")?, &run("--numstat")?, "");
        let mut text = format!("Changed files ({}):\n", files.len());
        for file in files {
            text.push_str(&format!(
                "{} {} (+{} -{})\n",
                file.status.code(),
                file.path,
                file.additions,
                file.deletions
            ));
        }
        Ok(text)
    }

    fn get_diff(&self, args: DiffArgs) -> Result<Value, ToolError> {
        let list = self.file_list()?;
        if args.summary {
            return Ok(Value::String(list));
        }
        let mut patch = self.git_diff(&args.files)?;
        if patch.len() > MAX_DIFF_BYTES {
            let mut cut = MAX_DIFF_BYTES;
            while !patch.is_char_boundary(cut) {
                cut -= 1;
            }
            patch.truncate(cut);
            patch.push_str("\n(The diff is cut here. Call get_diff with `files` to read the rest.)\n");
        }
        Ok(Value::String(format!("{list}\n{patch}")))
    }

    fn in_scope(&self, id: ThreadId) -> bool {
        self.binding.threads.is_empty() || self.binding.threads.contains(&id)
    }

    fn thread(&self, raw: &Value) -> Result<ReviewThread, ToolError> {
        let id =
            thread_id(raw).ok_or_else(|| ToolError::new("The thread id is not valid. Use an id from list_threads."))?;
        let found = self.store.load_thread(id)?;
        match found {
            Some((session, thread)) if session == self.session() && self.in_scope(id) => Ok(thread),
            Some((session, _)) if session == self.session() => Err(ToolError::new(
                "That thread is not part of this run. Work on the threads you were given.",
            )),
            _ => Err(ToolError::new(format!("There is no thread {}.", id.0))),
        }
    }

    fn list_threads(&self, args: ListArgs) -> Result<Value, ToolError> {
        let wanted = args.status.as_deref().map(ThreadStatus::parse);
        let threads = self.store.load_threads(self.session())?.value;
        let listed: Vec<Value> = threads
            .iter()
            .filter(|thread| self.in_scope(thread.id))
            .filter(|thread| thread.sent_comments().next().is_some())
            .filter(|thread| wanted.is_none_or(|status| thread.status == status))
            .map(thread_json)
            .collect();
        Ok(json!({ "threads": listed }))
    }

    fn diff_files(&self) -> Result<Vec<FileDiff>, ToolError> {
        Ok(diff::parse(&self.git_diff(&[])?))
    }

    fn add_comment(&self, args: CommentArgs) -> Result<Value, ToolError> {
        let path = args.file.trim().trim_start_matches("./").to_string();
        let side = match args.side.as_deref() {
            Some("old") => Side::Old,
            Some("new") | None => Side::New,
            Some(other) => {
                return Err(ToolError::new(format!(
                    "`side` must be \"new\" or \"old\", not \"{other}\"."
                )));
            }
        };
        let severity = match args.severity.as_deref() {
            None => Severity::Suggestion,
            Some(text) => Severity::parse(text)
                .ok_or_else(|| ToolError::new("`severity` must be must-fix, suggestion, nit or question."))?,
        };
        if args.body.trim().is_empty() {
            return Err(ToolError::new("The comment body is empty."));
        }
        let files = self.diff_files()?;
        let file = files.iter().find(|file| file.path == path).ok_or_else(|| {
            ToolError::new(format!(
                "`{path}` is not part of the diff. Call get_diff to see the files."
            ))
        })?;
        let end = args
            .end_line
            .filter(|end| *end >= args.start_line)
            .unwrap_or(args.start_line);
        let (anchor, end_line, anchor_text) = if args.start_line == 0 {
            (ThreadAnchor::File { path: path.clone() }, None, None)
        } else {
            let text = anchor::lines_text(file, side, args.start_line, end).ok_or_else(|| {
                ToolError::new(format!(
                    "Lines {}-{} of `{path}` are not in the diff on the {} side. Pick lines inside a hunk, as the @@ headers show.",
                    args.start_line,
                    end,
                    if side == Side::New { "new" } else { "old" }
                ))
            })?;
            (
                ThreadAnchor::Line {
                    path: path.clone(),
                    side,
                    line: args.start_line,
                },
                (end > args.start_line).then_some(end),
                Some(text),
            )
        };
        let title = args
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map_or_else(|| first_sentence(&args.body), String::from);
        let written = self.store.insert_thread(
            self.session(),
            &NewThread {
                anchor,
                end_line,
                anchor_text,
                finding: Some(FindingMeta {
                    severity,
                    title,
                    location: None,
                }),
                first_comment: Some(claude_comment(&args.body)),
            },
        )?;
        self.notify(Notice::ThreadsChanged);
        Ok(json!({ "threadId": written.value.id.0.to_string() }))
    }

    fn add_general_comment(&self, args: GeneralArgs) -> Result<Value, ToolError> {
        if args.body.trim().is_empty() {
            return Err(ToolError::new("The comment body is empty."));
        }
        let written = self.store.insert_thread(
            self.session(),
            &NewThread {
                first_comment: Some(claude_comment(&args.body)),
                ..NewThread::at(ThreadAnchor::Change)
            },
        )?;
        self.notify(Notice::ThreadsChanged);
        Ok(json!({ "threadId": written.value.id.0.to_string() }))
    }

    fn reply(&self, args: ReplyArgs) -> Result<Value, ToolError> {
        let thread = self.thread(&args.thread_id)?;
        if args.body.trim().is_empty() {
            return Err(ToolError::new("The reply is empty."));
        }
        self.store.insert_comment(thread.id, &claude_comment(&args.body))?;
        self.notify(Notice::ThreadsChanged);
        self.notify(Notice::ThreadAnswered(thread.id));
        Ok(json!({ "ok": true }))
    }

    fn resolve(&self, args: ResolveArgs) -> Result<Value, ToolError> {
        let thread = self.thread(&args.thread_id)?;
        let summary = args.summary.as_deref().map(str::trim).unwrap_or_default().to_string();
        if !summary.is_empty() {
            self.store.insert_comment(thread.id, &claude_comment(&summary))?;
        }
        self.store.mark_addressed(
            thread.id,
            &Addressed {
                run: self.binding.run,
                summary,
                files: args
                    .files
                    .iter()
                    .map(|file| file.trim().to_string())
                    .filter(|file| !file.is_empty())
                    .collect(),
            },
        )?;
        self.notify(Notice::ThreadsChanged);
        self.notify(Notice::ThreadAnswered(thread.id));
        Ok(json!({ "ok": true, "status": "addressed" }))
    }

    fn dismiss(&self, args: DismissArgs) -> Result<Value, ToolError> {
        let thread = self.thread(&args.thread_id)?;
        if let Some(reason) = args
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
        {
            self.store.insert_comment(thread.id, &claude_comment(reason))?;
        }
        self.store.set_status(thread.id, ThreadStatus::Dismissed)?;
        self.notify(Notice::ThreadsChanged);
        self.notify(Notice::ThreadAnswered(thread.id));
        Ok(json!({ "ok": true, "status": "dismissed" }))
    }
}

pub fn thread_json(thread: &ReviewThread) -> Value {
    let (path, side, start) = match &thread.anchor {
        ThreadAnchor::File { path } => (path.as_str(), None, 0),
        ThreadAnchor::Line { path, side, line } => (path.as_str(), Some(*side), *line),
        ThreadAnchor::Change => (GENERAL_PATH, None, 0),
    };
    let comments: Vec<Value> = thread
        .sent_comments()
        .map(|comment| {
            json!({
                "authorType": if comment.source == CommentSource::User { "user" } else { "agent" },
                "authorName": comment.author,
                "body": comment.body,
                "fromTerminal": comment.source == CommentSource::Terminal,
            })
        })
        .collect();
    json!({
        "id": thread.id.0.to_string(),
        "filePath": path,
        "startLine": start,
        "endLine": thread.end_line.unwrap_or(start),
        "side": side.map(|side| if side == Side::Old { "old" } else { "new" }),
        "status": thread.status.as_str(),
        "severity": thread.finding.as_ref().map(|finding| finding.severity.as_str()),
        "title": thread.finding.as_ref().map(|finding| finding.title.as_str()),
        "addressedBy": thread.addressed.as_ref().map(|addressed| json!({
            "agent": CLAUDE_NAME,
            "summary": addressed.summary,
            "files": addressed.files,
        })),
        "comments": comments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_names_are_the_contract() {
        let names: Vec<&str> = definitions().iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            [
                "get_diff",
                "list_threads",
                "add_comment",
                "add_general_comment",
                "reply",
                "resolve",
                "dismiss",
                "approve"
            ]
        );
        assert!(definitions().iter().all(|tool| tool.input_schema["type"] == "object"));
    }

    #[test]
    fn thread_ids_are_read_from_text_or_numbers() {
        assert_eq!(thread_id(&json!("12")), Some(ThreadId(12)));
        assert_eq!(thread_id(&json!(12)), Some(ThreadId(12)));
        assert_eq!(thread_id(&json!(" 7 ")), Some(ThreadId(7)));
        assert_eq!(thread_id(&json!("abc")), None);
        assert_eq!(thread_id(&json!(-1)), None);
        assert_eq!(thread_id(&Value::Null), None);
    }

    #[test]
    fn a_title_is_the_first_sentence_of_the_body() {
        assert_eq!(first_sentence("Null check missing. Add one."), "Null check missing.");
        assert_eq!(first_sentence("No full stop here\nsecond line"), "No full stop here");
        let long = "word ".repeat(40);
        assert!(first_sentence(&long).ends_with('…'));
        assert!(first_sentence(&long).chars().count() <= 81);
    }
}
