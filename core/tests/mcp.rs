mod common;

use common::*;
use diffpilot_core::mcp::{Binding, McpBridge, Notice, PermissionRequest};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::run::{RunId, RunKind};
use diffpilot_core::store::{NewComment, NewThread, SessionId, SessionKey, Store};
use diffpilot_core::threads::{CommentSource, Side, ThreadAnchor, ThreadId, ThreadStatus};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Fixture {
    repo: tempfile::TempDir,
    store: Arc<Store>,
    session: SessionId,
    bridge: Arc<McpBridge>,
    _socket_dir: tempfile::TempDir,
    notices: Arc<Mutex<Vec<Notice>>>,
}

fn fixture() -> Fixture {
    let repo = temp_repo();
    std::fs::write(
        repo.path().join("a.ts"),
        "const count = 1;\nexport { count };\nexport const extra = 3;\n",
    )
    .unwrap();
    std::fs::write(repo.path().join("new.ts"), "export const fresh = 1;\n").unwrap();
    git(repo.path(), &["add", "-A"]);
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = store.session(&key(repo.path())).unwrap();
    let socket_dir = tempfile::tempdir().unwrap();
    let bridge = McpBridge::start(socket_dir.path().join("mcp.sock"), store.clone()).unwrap();
    Fixture {
        repo,
        store,
        session,
        bridge,
        _socket_dir: socket_dir,
        notices: Arc::default(),
    }
}

impl Fixture {
    fn binding(&self, kind: RunKind, threads: Vec<ThreadId>) -> Binding {
        let notices = self.notices.clone();
        Binding {
            run: RunId(5),
            session: self.session,
            repo: self.repo.path().to_path_buf(),
            kind,
            revisions: vec!["--cached".into()],
            linked: Vec::new(),
            threads,
            permission: EditPermission::DontAsk,
            notify: Arc::new(move |notice| notices.lock().unwrap().push(notice)),
        }
    }

    fn token(&self, kind: RunKind, threads: Vec<ThreadId>) -> String {
        self.bridge.register(self.binding(kind, threads))
    }

    fn call(&self, token: &str, tool: &str, args: Value) -> Result<Value, String> {
        self.bridge.call(token, tool, args).map_err(|error| error.0)
    }

    fn user_thread(&self, line: u32, text: &str) -> ThreadId {
        let written = self
            .store
            .insert_thread(
                self.session,
                &NewThread {
                    anchor: ThreadAnchor::line("a.ts", Side::New, line),
                    end_line: None,
                    anchor_text: None,
                    finding: None,
                    first_comment: Some(NewComment {
                        author: "You".into(),
                        source: CommentSource::User,
                        body: text.into(),
                        streaming: false,
                        pending: false,
                    }),
                },
            )
            .unwrap();
        written.value.id
    }

    fn thread(&self, id: ThreadId) -> diffpilot_core::threads::ReviewThread {
        self.store.load_thread(id).unwrap().unwrap().1
    }
}

#[test]
fn get_diff_lists_the_files_then_the_patch_and_can_limit_both() {
    let fx = fixture();
    let token = fx.token(RunKind::Review, vec![]);
    let all = fx.call(&token, "get_diff", json!({})).unwrap();
    let text = all.as_str().unwrap();
    assert!(text.starts_with("Changed files (2):\n"), "{text}");
    assert!(text.contains("M a.ts (+2 -1)") || text.contains("M a.ts"), "{text}");
    assert!(text.contains("A new.ts (+1 -0)"), "{text}");
    assert!(text.contains("@@ "));
    assert!(text.contains("+export const extra = 3;"));

    let summary = fx.call(&token, "get_diff", json!({ "summary": true })).unwrap();
    assert!(!summary.as_str().unwrap().contains("@@"));
    let one = fx.call(&token, "get_diff", json!({ "files": ["new.ts"] })).unwrap();
    assert!(one.as_str().unwrap().contains("+export const fresh = 1;"));
    assert!(!one.as_str().unwrap().contains("extra"));
}

#[test]
fn add_comment_makes_a_finding_thread_anchored_to_the_diff() {
    let fx = fixture();
    let token = fx.token(RunKind::Review, vec![]);
    let made = fx
        .call(
            &token,
            "add_comment",
            json!({ "file": "a.ts", "startLine": 2, "endLine": 3, "body": "Unused export. Drop it.", "severity": "nit" }),
        )
        .unwrap();
    let id = ThreadId(made["threadId"].as_str().unwrap().parse().unwrap());
    let thread = fx.thread(id);
    assert_eq!(thread.anchor, ThreadAnchor::line("a.ts", Side::New, 2));
    assert_eq!(thread.end_line, Some(3));
    assert_eq!(
        thread.anchor_text.as_deref(),
        Some("export { count };\nexport const extra = 3;")
    );
    let finding = thread.finding.unwrap();
    assert_eq!(finding.severity.as_str(), "nit");
    assert_eq!(finding.title, "Unused export.");
    assert_eq!(
        (thread.comments[0].author.as_str(), thread.comments[0].source),
        ("Claude", CommentSource::Claude)
    );
    assert_eq!(fx.notices.lock().unwrap().as_slice(), [Notice::ThreadsChanged]);

    let file_level = fx
        .call(&token, "add_comment", json!({ "file": "new.ts", "startLine": 0, "body": "Whole file.", "title": "Name it", "severity": "question" }))
        .unwrap();
    let thread = fx.thread(ThreadId(file_level["threadId"].as_str().unwrap().parse().unwrap()));
    assert_eq!(thread.anchor, ThreadAnchor::file("new.ts"));
    assert_eq!(thread.finding.unwrap().title, "Name it");

    let general = fx
        .call(&token, "add_general_comment", json!({ "body": "Looks fine." }))
        .unwrap();
    let thread = fx.thread(ThreadId(general["threadId"].as_str().unwrap().parse().unwrap()));
    assert_eq!(thread.anchor, ThreadAnchor::Change);
    assert!(thread.finding.is_none());
}

#[test]
fn add_comment_refuses_lines_and_files_outside_the_diff() {
    let fx = fixture();
    let token = fx.token(RunKind::Review, vec![]);
    let bad = |args: Value| fx.call(&token, "add_comment", args).unwrap_err();
    assert!(
        bad(json!({ "file": "b.ts", "startLine": 1, "body": "x", "severity": "nit" })).contains("not part of the diff")
    );
    assert!(
        bad(json!({ "file": "a.ts", "startLine": 99, "body": "x", "severity": "nit" })).contains("not in the diff")
    );
    assert!(
        bad(json!({ "file": "a.ts", "startLine": 50, "side": "old", "body": "x", "severity": "nit" }))
            .contains("old side")
    );
    assert!(bad(json!({ "file": "a.ts", "startLine": 1, "body": "x", "severity": "huge" })).contains("severity"));
    assert!(bad(json!({ "file": "a.ts", "startLine": 1, "body": "  ", "severity": "nit" })).contains("empty"));
    assert!(bad(json!({ "file": "a.ts" })).contains("Bad arguments"));
    assert!(fx.store.load_threads(fx.session).unwrap().value.is_empty());
}

#[test]
fn list_threads_shows_what_a_run_may_see_and_hides_pending_comments() {
    let fx = fixture();
    let first = fx.user_thread(1, "Rename this.");
    let second = fx.user_thread(2, "And this?");
    let draft = fx.user_thread(3, "Unsent draft");
    fx.store
        .insert_comment(
            draft,
            &NewComment {
                author: "You".into(),
                source: CommentSource::User,
                body: "x".into(),
                streaming: false,
                pending: true,
            },
        )
        .unwrap();
    fx.store.set_status(second, ThreadStatus::Resolved).unwrap();
    let pending_only = fx
        .store
        .insert_thread(
            fx.session,
            &NewThread {
                first_comment: Some(NewComment {
                    author: "You".into(),
                    source: CommentSource::User,
                    body: "secret".into(),
                    streaming: false,
                    pending: true,
                }),
                ..NewThread::at(ThreadAnchor::file("a.ts"))
            },
        )
        .unwrap()
        .value
        .id;

    let token = fx.token(RunKind::MakeChanges, vec![]);
    let listed = fx.call(&token, "list_threads", json!({})).unwrap();
    let ids: Vec<&str> = listed["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [first.0.to_string(), second.0.to_string(), draft.0.to_string()]);
    assert!(!listed.to_string().contains("secret"));
    assert!(!ids.contains(&pending_only.0.to_string().as_str()));
    let draft_json = &listed["threads"][2];
    assert_eq!(
        draft_json["comments"].as_array().unwrap().len(),
        1,
        "the pending comment is not sent"
    );
    let open = fx.call(&token, "list_threads", json!({ "status": "open" })).unwrap();
    assert_eq!(open["threads"].as_array().unwrap().len(), 2);
    let one = &listed["threads"][0];
    assert_eq!(
        (
            one["filePath"].as_str(),
            one["startLine"].as_u64(),
            one["side"].as_str()
        ),
        (Some("a.ts"), Some(1), Some("new"))
    );
    assert_eq!(one["comments"][0]["authorType"], "user");

    let scoped = fx.token(RunKind::FixComments, vec![first]);
    let only = fx.call(&scoped, "list_threads", json!({})).unwrap();
    assert_eq!(only["threads"].as_array().unwrap().len(), 1);
}

#[test]
fn reply_resolve_and_dismiss_record_what_claude_did() {
    let fx = fixture();
    let (a, b, c) = (
        fx.user_thread(1, "Fix A"),
        fx.user_thread(2, "Fix B"),
        fx.user_thread(3, "Not needed"),
    );
    let token = fx.token(RunKind::FixComments, vec![a, b, c]);

    fx.call(
        &token,
        "reply",
        json!({ "threadId": a.0.to_string(), "body": "On it." }),
    )
    .unwrap();
    let resolved = fx
        .call(
            &token,
            "resolve",
            json!({ "threadId": a.0, "summary": "Fixed: renamed.", "files": ["a.ts", " "] }),
        )
        .unwrap();
    assert_eq!(resolved["status"], "addressed");
    let thread = fx.thread(a);
    assert_eq!(thread.status, ThreadStatus::Open, "the reviewer closes it, not Claude");
    let addressed = thread.addressed.unwrap();
    assert_eq!(
        (addressed.run, addressed.summary.as_str(), addressed.files.as_slice()),
        (RunId(5), "Fixed: renamed.", ["a.ts".to_string()].as_slice())
    );
    let bodies: Vec<_> = thread.comments.iter().map(|c| c.body.as_str()).collect();
    assert_eq!(bodies, ["Fix A", "On it.", "Fixed: renamed."]);

    fx.call(
        &token,
        "dismiss",
        json!({ "threadId": c.0.to_string(), "reason": "Already done upstream." }),
    )
    .unwrap();
    let dismissed = fx.thread(c);
    assert_eq!(dismissed.status, ThreadStatus::Dismissed);
    assert_eq!(dismissed.comments.last().unwrap().body, "Already done upstream.");

    let notices = fx.notices.lock().unwrap().clone();
    assert!(notices.contains(&Notice::ThreadAnswered(a)) && notices.contains(&Notice::ThreadAnswered(c)));
    assert!(!notices.contains(&Notice::ThreadAnswered(b)));
}

#[test]
fn a_run_cannot_touch_threads_outside_its_scope_or_in_another_review() {
    let fx = fixture();
    let mine = fx.user_thread(1, "Mine");
    let other = fx.user_thread(2, "Not in this run");
    let other_review = {
        let session = fx.store.session(&SessionKey::new("/elsewhere", "x", "main")).unwrap();
        fx.store
            .insert_thread(session, &NewThread::at(ThreadAnchor::Change))
            .unwrap()
            .value
            .id
    };
    let token = fx.token(RunKind::FixComments, vec![mine]);
    let reply = |id: ThreadId| fx.call(&token, "reply", json!({ "threadId": id.0.to_string(), "body": "x" }));
    assert!(reply(other).unwrap_err().contains("not part of this run"));
    assert!(reply(other_review).unwrap_err().contains("no thread"));
    assert!(
        fx.call(&token, "resolve", json!({ "threadId": "nope" }))
            .unwrap_err()
            .contains("not valid")
    );
    assert!(reply(mine).is_ok());
    assert_eq!(fx.thread(other).comments.len(), 1);
}

#[test]
fn each_kind_of_run_gets_only_its_own_tools() {
    let fx = fixture();
    let names = |kind| -> Vec<String> {
        let token = fx.token(kind, vec![]);
        serde_json::from_value(fx.call(&token, "__list_tools", Value::Null).unwrap()).unwrap()
    };
    assert_eq!(
        names(RunKind::Review),
        ["get_diff", "list_threads", "add_comment", "add_general_comment"]
    );
    assert_eq!(
        names(RunKind::FixComments),
        ["get_diff", "list_threads", "reply", "resolve", "dismiss"]
    );
    assert_eq!(names(RunKind::MakeChanges), ["get_diff", "list_threads"]);
    let review = fx.token(RunKind::Review, vec![]);
    assert!(
        fx.call(&review, "resolve", json!({ "threadId": "1" }))
            .unwrap_err()
            .contains("not available in a review run")
    );
    let fix = fx.token(RunKind::FixComments, vec![]);
    assert!(
        fx.call(&fix, "add_comment", json!({}))
            .unwrap_err()
            .contains("not available in a fix comments run")
    );
    assert!(
        fx.call(&fix, "approve", json!({}))
            .unwrap_err()
            .contains("not available")
    );
    assert!(
        fx.call(&fix, "delete_everything", json!({}))
            .unwrap_err()
            .contains("not available")
    );
}

#[test]
fn a_token_stops_working_when_the_run_is_over() {
    let fx = fixture();
    let token = fx.token(RunKind::Review, vec![]);
    assert!(fx.call(&token, "get_diff", json!({ "summary": true })).is_ok());
    fx.bridge.unregister(&token);
    assert!(fx.call(&token, "get_diff", json!({})).unwrap_err().contains("over"));
    assert!(fx.call("not-a-token", "get_diff", json!({})).is_err());
}

fn ask_binding(fx: &Fixture, permission: EditPermission, requests: Arc<Mutex<Vec<PermissionRequest>>>) -> String {
    let mut binding = fx.binding(RunKind::FixComments, vec![]);
    binding.permission = permission;
    binding.notify = Arc::new(move |notice| {
        if let Notice::Permission(request) = notice {
            requests.lock().unwrap().push(request);
        }
    });
    fx.bridge.register(binding)
}

fn approve(fx: &Fixture, token: &str, tool: &str, input: Value) -> Value {
    let answer = fx
        .call(token, "approve", json!({ "tool_name": tool, "input": input }))
        .unwrap();
    serde_json::from_str(answer.as_str().unwrap()).unwrap()
}

fn answer_when_asked(fx: &Fixture, requests: &Arc<Mutex<Vec<PermissionRequest>>>, count: usize, allow: bool) {
    wait_for("a permission request", Duration::from_secs(10), || {
        requests.lock().unwrap().len() >= count
    });
    let id = requests.lock().unwrap()[count - 1].id;
    assert!(fx.bridge.answer_permission(id, allow));
}

#[test]
fn ask_every_edit_asks_each_time_and_the_answer_decides() {
    let fx = fixture();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let token = ask_binding(&fx, EditPermission::AskEach, requests.clone());
    let input =
        json!({ "file_path": format!("{}/a.ts", fx.repo.path().display()), "old_string": "a", "new_string": "b" });

    std::thread::scope(|scope| {
        let asker = scope.spawn(|| approve(&fx, &token, "Edit", input.clone()));
        answer_when_asked(&fx, &requests, 1, true);
        let answer = asker.join().unwrap();
        assert_eq!(answer["behavior"], "allow");
        assert_eq!(answer["updatedInput"], input);

        let asker = scope.spawn(|| approve(&fx, &token, "Edit", input.clone()));
        answer_when_asked(&fx, &requests, 2, false);
        let answer = asker.join().unwrap();
        assert_eq!(answer["behavior"], "deny");
        assert!(answer["message"].as_str().unwrap().contains("did not allow"));
    });
    let first = requests.lock().unwrap()[0].clone();
    assert_eq!(
        (first.tool.as_str(), first.summary.as_str(), first.is_edit),
        ("Edit", "Edit a.ts", true)
    );
    assert_eq!(first.run, RunId(5));
}

#[test]
fn ask_once_remembers_the_first_edit_but_still_asks_for_commands() {
    let fx = fixture();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let token = ask_binding(&fx, EditPermission::AskOnce, requests.clone());
    std::thread::scope(|scope| {
        let first = scope.spawn(|| approve(&fx, &token, "Write", json!({ "file_path": "/x/new.ts", "content": "" })));
        answer_when_asked(&fx, &requests, 1, true);
        assert_eq!(first.join().unwrap()["behavior"], "allow");

        let second = approve(&fx, &token, "Edit", json!({ "file_path": "/x/a.ts" }));
        assert_eq!(second["behavior"], "allow");
        assert_eq!(requests.lock().unwrap().len(), 1, "the second edit was not asked");

        let command = scope.spawn(|| approve(&fx, &token, "Bash", json!({ "command": "npm test\nmore" })));
        answer_when_asked(&fx, &requests, 2, true);
        assert_eq!(command.join().unwrap()["behavior"], "allow");
    });
    let asked = requests.lock().unwrap().clone();
    assert_eq!(
        (asked[1].tool.as_str(), asked[1].summary.as_str(), asked[1].is_edit),
        ("Bash", "Bash npm test", false)
    );
}

#[test]
fn a_refused_first_edit_is_not_remembered() {
    let fx = fixture();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let token = ask_binding(&fx, EditPermission::AskOnce, requests.clone());
    std::thread::scope(|scope| {
        let first = scope.spawn(|| approve(&fx, &token, "Edit", json!({ "file_path": "/x/a.ts" })));
        answer_when_asked(&fx, &requests, 1, false);
        assert_eq!(first.join().unwrap()["behavior"], "deny");
        let again = scope.spawn(|| approve(&fx, &token, "Edit", json!({ "file_path": "/x/a.ts" })));
        answer_when_asked(&fx, &requests, 2, true);
        assert_eq!(again.join().unwrap()["behavior"], "allow");
    });
}

#[test]
fn ending_the_run_denies_a_request_nobody_answered() {
    let fx = fixture();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let token = ask_binding(&fx, EditPermission::AskEach, requests.clone());
    std::thread::scope(|scope| {
        let waiting = scope.spawn(|| approve(&fx, &token, "Edit", json!({ "file_path": "/x/a.ts" })));
        wait_for("the request", Duration::from_secs(10), || {
            !requests.lock().unwrap().is_empty()
        });
        fx.bridge.unregister(&token);
        assert_eq!(waiting.join().unwrap()["behavior"], "deny");
    });
    assert!(!fx.bridge.answer_permission(requests.lock().unwrap()[0].id, true));
}

#[test]
fn dont_ask_runs_cannot_call_the_prompt_tool() {
    let fx = fixture();
    let token = fx.token(RunKind::FixComments, vec![]);
    assert!(
        fx.call(&token, "approve", json!({ "tool_name": "Edit", "input": {} }))
            .is_err()
    );
}

fn line_exchange(stream: &UnixStream, request: Value) -> Value {
    let mut writer = stream.try_clone().unwrap();
    writeln!(writer, "{request}").unwrap();
    let mut answer = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut answer)
        .unwrap();
    serde_json::from_str(&answer).unwrap()
}

#[test]
fn the_socket_answers_one_json_line_per_call_and_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture();
    let token = fx.token(RunKind::Review, vec![]);
    let socket = fx.bridge.socket_path().to_path_buf();
    assert_eq!(std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777, 0o600);

    let stream = UnixStream::connect(&socket).unwrap();
    let ok = line_exchange(&stream, json!({ "token": token, "tool": "list_threads", "args": {} }));
    assert_eq!(ok, json!({ "ok": true, "result": { "threads": [] } }));
    let bad_token = line_exchange(&stream, json!({ "token": "x", "tool": "list_threads" }));
    assert_eq!(bad_token["ok"], false);
    let mut writer = stream.try_clone().unwrap();
    writeln!(writer, "not json").unwrap();
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).unwrap();
    assert!(answer.contains("bad request"));
}

#[test]
fn dropping_the_bridge_removes_its_socket() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("mcp.sock");
    let bridge = McpBridge::start(socket.clone(), Arc::new(Store::open_in_memory().unwrap())).unwrap();
    assert!(socket.exists());
    drop(bridge);
    assert!(!socket.exists());
}

#[test]
fn resolve_fails_for_the_rest_of_a_run_after_a_refused_edit() {
    let fx = fixture();
    let thread = fx.user_thread(1, "Rename it.");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let token = ask_binding(&fx, EditPermission::AskEach, requests.clone());
    let resolve = || {
        fx.call(
            &token,
            "resolve",
            json!({ "threadId": thread.0.to_string(), "summary": "Fixed." }),
        )
    };
    std::thread::scope(|scope| {
        let first = scope.spawn(|| approve(&fx, &token, "Edit", json!({ "file_path": "/x/a.ts" })));
        answer_when_asked(&fx, &requests, 1, true);
        assert_eq!(first.join().unwrap()["behavior"], "allow");
        let second = scope.spawn(|| approve(&fx, &token, "Bash", json!({ "command": "npm test" })));
        answer_when_asked(&fx, &requests, 2, false);
        assert_eq!(second.join().unwrap()["behavior"], "deny");
        assert!(resolve().is_ok(), "a refused command is not a refused edit");
    });
    fx.store.clear_addressed(thread).unwrap();
    std::thread::scope(|scope| {
        let edit = scope.spawn(|| approve(&fx, &token, "Write", json!({ "file_path": "/x/b.ts", "content": "" })));
        answer_when_asked(&fx, &requests, 3, false);
        assert_eq!(edit.join().unwrap()["behavior"], "deny");
    });
    assert!(resolve().unwrap_err().starts_with("edit_rejected"));
    assert!(fx.store.load_thread(thread).unwrap().unwrap().1.addressed.is_none());
    assert!(
        fx.call(
            &token,
            "reply",
            json!({ "threadId": thread.0.to_string(), "body": "I would rename it." })
        )
        .is_ok()
    );
}
