//! The app side of the MCP server: a unix socket that `diffpilot-mcp` forwards tool calls to.
//!
//! Each run registers a `Binding`, and its token is the only way to the tools. A token names one run, so a call
//! can only reach the review, the threads and the tools that run was given. diffity's bridge works the same
//! way, with tokio (MIT, see THIRD_PARTY_NOTICES).

use super::protocol::LIST_TOOLS;
use super::tools::{self, ToolError};
use crate::policy::{APPROVE_TOOL, EditPermission};
use crate::run::RunKind;
use crate::store::{RunId, SessionId, Store};
use crate::threads::ThreadId;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, Weak};

const MAX_SOCKET_PATH: usize = 100;

/// What the bridge tells the app while a run works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    ThreadsChanged,
    /// The run added to the guide it is writing.
    GuideChanged,
    ThreadAnswered(ThreadId),
    Permission(PermissionRequest),
}

/// Claude wants to use `tool` and the permission setting says to ask. Answer with `McpBridge::answer_permission`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub id: u64,
    pub run: RunId,
    pub tool: String,
    pub summary: String,
    pub paths: Vec<String>,
    pub is_edit: bool,
}

#[derive(Clone)]
pub struct Binding {
    pub run: RunId,
    pub session: SessionId,
    pub repo: PathBuf,
    pub kind: RunKind,
    /// The `git diff` revisions of the comparison under review.
    pub revisions: Vec<String>,
    /// Folders outside the repo the run may read. A guide's impact may point into them.
    pub linked: Vec<PathBuf>,
    /// The threads this run may read and answer. Empty means every thread of the review.
    pub threads: Vec<ThreadId>,
    /// How the run may edit files, None when it may not.
    pub edits: Option<EditPermission>,
    pub notify: Arc<dyn Fn(Notice) + Send + Sync>,
}

struct Bound {
    binding: Binding,
    edits_approved: AtomicBool,
    edit_rejected: AtomicBool,
}

pub struct McpBridge {
    socket_path: PathBuf,
    store: Arc<Store>,
    bindings: Mutex<HashMap<String, Arc<Bound>>>,
    pending: Mutex<HashMap<u64, (RunId, Sender<bool>)>>,
    next_request: AtomicU64,
}

/// The data folder's `mcp.sock`, or `/tmp/diffpilot-<uid>.sock` when that path is too long for a unix socket.
pub fn socket_path_for(data_dir: &Path) -> PathBuf {
    let preferred = data_dir.join("mcp.sock");
    if preferred.as_os_str().len() <= MAX_SOCKET_PATH {
        return preferred;
    }
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/diffpilot-{uid}.sock"))
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 24];
    let filled = std::fs::File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes));
    if filled.is_err() {
        use sha2::{Digest, Sha256};
        let seed = format!("{:?}-{}-{:p}", std::time::SystemTime::now(), std::process::id(), &bytes);
        bytes.copy_from_slice(&Sha256::digest(seed.as_bytes())[..24]);
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Deserialize)]
struct Request {
    token: String,
    tool: String,
    #[serde(default)]
    args: Value,
}

impl McpBridge {
    pub fn start(socket_path: PathBuf, store: Arc<Store>) -> std::io::Result<Arc<Self>> {
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)?;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
        let bridge = Arc::new(Self {
            socket_path,
            store,
            bindings: Mutex::default(),
            pending: Mutex::default(),
            next_request: AtomicU64::new(1),
        });
        let weak = Arc::downgrade(&bridge);
        std::thread::Builder::new()
            .name("diffpilot-mcp-bridge".into())
            .spawn(move || accept_loop(&listener, &weak))?;
        Ok(bridge)
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn register(&self, binding: Binding) -> String {
        let token = random_token();
        self.bindings.lock().unwrap().insert(
            token.clone(),
            Arc::new(Bound {
                binding,
                edits_approved: AtomicBool::new(false),
                edit_rejected: AtomicBool::new(false),
            }),
        );
        token
    }

    /// The token stops working, and a permission request still waiting for an answer is denied.
    pub fn unregister(&self, token: &str) {
        let Some(bound) = self.bindings.lock().unwrap().remove(token) else {
            return;
        };
        self.pending
            .lock()
            .unwrap()
            .retain(|_, (run, _)| *run != bound.binding.run);
    }

    /// Answers a `Notice::Permission`. False when the request is gone, for example because the run ended.
    pub fn answer_permission(&self, id: u64, allow: bool) -> bool {
        let waiting = self.pending.lock().unwrap().remove(&id);
        waiting.is_some_and(|(_, sender)| sender.send(allow).is_ok())
    }

    pub fn call(&self, token: &str, tool: &str, args: Value) -> Result<Value, ToolError> {
        let bound = self
            .bindings
            .lock()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or_else(|| ToolError("This run is over, so its tools no longer work.".into()))?;
        if tool == LIST_TOOLS {
            return Ok(json!(tools::allowed(&bound.binding)));
        }
        if tool == APPROVE_TOOL && tools::allowed(&bound.binding).contains(&APPROVE_TOOL) {
            return Ok(self.approve(&bound, &args));
        }
        if tool == "resolve" && bound.edit_rejected.load(Ordering::SeqCst) {
            return Err(ToolError(
                "edit_rejected: The reviewer rejected an edit in this run. Reply on the thread instead of resolving it."
                    .into(),
            ));
        }
        tools::call(&self.store, &bound.binding, tool, args)
    }

    /// The answer Claude Code expects from a `--permission-prompt-tool`.
    fn approve(&self, bound: &Bound, args: &Value) -> Value {
        let tool = args.get("tool_name").and_then(Value::as_str).unwrap_or_default();
        let input = args.get("input").cloned().unwrap_or(Value::Null);
        let is_edit = matches!(tool, "Edit" | "Write" | "MultiEdit" | "NotebookEdit");
        let binding = &bound.binding;
        let remembered =
            binding.edits == Some(EditPermission::AskOnce) && is_edit && bound.edits_approved.load(Ordering::SeqCst);
        let allow = remembered || self.ask(binding, tool, &input, is_edit);
        if allow && is_edit && binding.edits == Some(EditPermission::AskOnce) {
            bound.edits_approved.store(true, Ordering::SeqCst);
        }
        if !allow && is_edit {
            bound.edit_rejected.store(true, Ordering::SeqCst);
        }
        let text = if allow {
            json!({ "behavior": "allow", "updatedInput": input })
        } else {
            json!({ "behavior": "deny", "message": "The reviewer did not allow this. Do not retry it. Reply on the thread to say what you would change instead." })
        };
        Value::String(text.to_string())
    }

    fn ask(&self, binding: &Binding, tool: &str, input: &Value, is_edit: bool) -> bool {
        let id = self.next_request.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = channel();
        self.pending.lock().unwrap().insert(id, (binding.run, sender));
        let path = ["file_path", "notebook_path", "path"]
            .iter()
            .find_map(|key| input.get(key).and_then(Value::as_str))
            .map(String::from);
        let summary = match (&path, input.get("command").and_then(Value::as_str)) {
            (Some(path), _) => format!(
                "{tool} {}",
                path.strip_prefix(&*binding.repo.to_string_lossy())
                    .unwrap_or(path)
                    .trim_start_matches('/')
            ),
            (None, Some(command)) => format!("{tool} {}", command.lines().next().unwrap_or_default()),
            (None, None) => tool.to_string(),
        };
        (binding.notify)(Notice::Permission(PermissionRequest {
            id,
            run: binding.run,
            tool: tool.to_string(),
            summary,
            paths: path.into_iter().collect(),
            is_edit,
        }));
        receiver.recv().unwrap_or(false)
    }

    /// One line of the socket protocol in, one line out.
    pub fn handle_line(&self, line: &str) -> Value {
        let request: Request = match serde_json::from_str(line) {
            Ok(request) => request,
            Err(error) => return json!({ "ok": false, "error": format!("bad request: {error}") }),
        };
        match self.call(&request.token, &request.tool, request.args) {
            Ok(result) => json!({ "ok": true, "result": result }),
            Err(error) => json!({ "ok": false, "error": error.0 }),
        }
    }

    fn serve(&self, stream: UnixStream) {
        let Ok(reader) = stream.try_clone() else { return };
        let mut writer = stream;
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { return };
            if line.trim().is_empty() {
                continue;
            }
            let mut answer = self.handle_line(&line).to_string();
            answer.push('\n');
            if writer.write_all(answer.as_bytes()).is_err() {
                return;
            }
        }
    }
}

fn accept_loop(listener: &UnixListener, bridge: &Weak<McpBridge>) {
    for stream in listener.incoming() {
        let Some(bridge) = bridge.upgrade() else { return };
        let Ok(stream) = stream else { continue };
        let _ = std::thread::Builder::new()
            .name("diffpilot-mcp-call".into())
            .spawn(move || bridge.serve(stream));
    }
}

impl Drop for McpBridge {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
        // Wakes the accept loop, which then finds the bridge gone and ends.
        let _ = UnixStream::connect(&self.socket_path);
    }
}
