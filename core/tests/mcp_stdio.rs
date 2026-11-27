mod common;

use common::*;
use diffpilot_core::mcp::{Binding, McpBridge};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::run::{RunId, RunKind};
use diffpilot_core::store::{NewThread, Store};
use diffpilot_core::threads::ThreadAnchor;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Server {
    fn start(socket: &std::path::Path, token: &str) -> Self {
        let mut child = Command::new(bin("diffpilot-mcp"))
            .args(["--socket", &socket.to_string_lossy(), "--token", token])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self { child, stdin, stdout }
    }

    fn send(&mut self, message: Value) {
        writeln!(self.stdin.as_mut().unwrap(), "{message}").unwrap();
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|error| panic!("{error}: {line:?}"))
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        self.read()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

#[test]
fn the_binary_speaks_mcp_over_stdio_and_forwards_to_the_app() {
    let repo = temp_repo();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = store.session(&key(repo.path())).unwrap();
    let thread = store
        .insert_thread(session, &NewThread::at(ThreadAnchor::file("a.ts")))
        .unwrap()
        .value
        .id;
    store
        .insert_comment(
            thread,
            &diffpilot_core::store::NewComment {
                author: "You".into(),
                source: diffpilot_core::threads::CommentSource::User,
                body: "Please rename".into(),
                streaming: false,
                pending: false,
            },
        )
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let bridge = McpBridge::start(dir.path().join("mcp.sock"), store.clone()).unwrap();
    let token = bridge.register(Binding {
        run: RunId(1),
        session,
        repo: repo.path().to_path_buf(),
        kind: RunKind::FixComments,
        revisions: vec!["HEAD".into()],
        linked: Vec::new(),
        threads: vec![thread],
        edits: Some(EditPermission::DontAsk),
        notify: Arc::new(|_| {}),
    });

    let mut server = Server::start(bridge.socket_path(), &token);
    let init = server.request(
        1,
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "diffpilot");
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    server.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));

    let listed = server.request(2, "tools/list", json!({}));
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["get_diff", "list_threads", "reply", "resolve", "dismiss"]);

    let threads = server.request(3, "tools/call", json!({ "name": "list_threads", "arguments": {} }));
    assert_eq!(threads["result"]["isError"], false);
    let text = threads["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["threads"][0]["comments"][0]["body"], "Please rename");

    let replied = server.request(
        4,
        "tools/call",
        json!({ "name": "reply", "arguments": { "threadId": thread.0.to_string(), "body": "Done." } }),
    );
    assert_eq!(replied["result"]["isError"], false);
    assert_eq!(store.load_thread(thread).unwrap().unwrap().1.comments.len(), 2);

    let refused = server.request(5, "tools/call", json!({ "name": "add_comment", "arguments": {} }));
    assert_eq!(refused["result"]["isError"], true);
    assert!(
        refused["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not available")
    );
    assert_eq!(server.request(6, "ping", json!({}))["result"], json!({}));
    assert_eq!(server.request(7, "nope", json!({}))["error"]["code"], -32601);
}

#[test]
fn the_binary_reports_an_unreachable_app_as_a_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = Server::start(&dir.path().join("missing.sock"), "token");
    let answer = server.request(1, "tools/call", json!({ "name": "list_threads", "arguments": {} }));
    assert_eq!(answer["result"]["isError"], true);
    assert!(
        answer["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not reachable")
    );
    let listed = server.request(2, "tools/list", json!({}));
    assert!(!listed["result"]["tools"].as_array().unwrap().is_empty());
}

#[test]
fn the_binary_needs_a_socket_and_a_token() {
    let status = Command::new(bin("diffpilot-mcp"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}
