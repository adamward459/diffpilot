mod common;

use common::*;
use diffpilot_core::fix::{self, Skip, Target};
use diffpilot_core::mcp::McpBridge;
use diffpilot_core::policy::EditPermission;
use diffpilot_core::review::{Effort, ModelName, ReviewModel, ReviewRequest};
use diffpilot_core::run::{Progress, RunKind, RunOutcome, RunState};
use diffpilot_core::runner::{McpLaunch, RunEngine, RunEvent, RunRequest};
use diffpilot_core::store::{NewComment, NewThread, SessionId, Store};
use diffpilot_core::threads::{CommentSource, Side, ThreadAnchor, ThreadId, ThreadStatus};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const WITHIN: Duration = Duration::from_secs(30);

struct World {
    repo: tempfile::TempDir,
    work: tempfile::TempDir,
    store: Arc<Store>,
    session: SessionId,
    engine: RunEngine,
    bridge: Arc<McpBridge>,
}

fn out(work: &Path) -> PathBuf {
    work.join("out")
}

fn mcp_claude(work: &Path, before: &str, calls: &[Value], after: &str) -> PathBuf {
    let out = out(work);
    std::fs::create_dir_all(&out).unwrap();
    let mut lines = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"fake-claude","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    ];
    for (index, call) in calls.iter().enumerate() {
        lines.push(json!({"jsonrpc":"2.0","id":10 + index,"method":"tools/call","params":call}));
    }
    let printf: String = lines
        .iter()
        .map(|line| format!("  printf '%s\\n' '{line}'\n"))
        .collect();
    fake_claude(
        work,
        &format!(
            r#"
for arg in "$@"; do printf '%s\n' "$arg" >> '{out}/args.txt'; done
CFG=''
while [ $# -gt 0 ]; do
  if [ "$1" = "--mcp-config" ]; then CFG="$2"; fi
  shift
done
cp "$CFG" '{out}/mcp.json'
CMD=$(sed -n 's/^ *"command": "\(.*\)",\{{0,1\}}$/\1/p' "$CFG")
SOCK=$(sed -n '/"--socket"/{{n;s/^ *"\(.*\)",\{{0,1\}}$/\1/p;}}' "$CFG")
TOKEN=$(sed -n '/"--token"/{{n;s/^ *"\(.*\)",\{{0,1\}}$/\1/p;}}' "$CFG")
echo '{{"type":"system","subtype":"init","session_id":"s-1","model":"m","tools":[],"mcp_servers":[{{"name":"diffpilot","status":"connected"}}]}}'
{before}
{{
{printf}}} | "$CMD" --socket "$SOCK" --token "$TOKEN" > '{out}/mcp.out'
{after}
echo '{{"type":"result","subtype":"success","is_error":false,"num_turns":3,"result":"All done."}}'
"#,
            out = out.display(),
        ),
    )
}

fn world(claude: impl FnOnce(&Path) -> PathBuf) -> World {
    let repo = temp_repo();
    let work = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = store.session(&key(repo.path())).unwrap();
    let bridge = McpBridge::start(work.path().join("mcp.sock"), store.clone()).unwrap();
    let mut config = config(store.clone(), claude(work.path()), work.path());
    let mut mcp = McpLaunch::new(bridge.clone());
    mcp.binary = PathBuf::from(env!("CARGO_BIN_EXE_diffpilot-mcp"));
    config.mcp = Some(mcp);
    World {
        engine: RunEngine::new(config),
        repo,
        work,
        store,
        session,
        bridge,
    }
}

impl World {
    fn comment(&self, line: u32, body: &str) -> ThreadId {
        self.comment_in("a.ts", line, body)
    }

    fn comment_in(&self, path: &str, line: u32, body: &str) -> ThreadId {
        self.store
            .insert_thread(
                self.session,
                &NewThread {
                    anchor: ThreadAnchor::line(path, Side::New, line),
                    end_line: None,
                    anchor_text: None,
                    finding: None,
                    first_comment: Some(NewComment {
                        author: "You".into(),
                        source: CommentSource::User,
                        body: body.into(),
                        streaming: false,
                        pending: false,
                    }),
                },
            )
            .unwrap()
            .value
            .id
    }

    fn target(&self, edit_permission: EditPermission) -> Target {
        Target {
            repo: self.repo.path().to_path_buf(),
            session: key(self.repo.path()),
            revisions: vec!["HEAD".into()],
            model: ReviewModel::new(ModelName::Sonnet, Effort::Low),
            edit_permission,
        }
    }

    fn threads(&self) -> Vec<diffpilot_core::threads::ReviewThread> {
        self.store.load_threads(self.session).unwrap().value
    }

    fn request(&self, kind: RunKind, threads: Vec<ThreadId>) -> RunRequest {
        let mut request = RunRequest::new(
            self.repo.path(),
            key(self.repo.path()),
            kind,
            "Fixing comments",
            "Fix the comments.",
            ReviewModel::new(ModelName::Sonnet, Effort::Medium),
        );
        request.threads = threads;
        request.revisions = vec!["HEAD".into()];
        request
    }

    fn mcp_out(&self) -> Vec<Value> {
        std::fs::read_to_string(out(self.work.path()).join("mcp.out"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn thread(&self, id: ThreadId) -> diffpilot_core::threads::ReviewThread {
        self.store.load_thread(id).unwrap().unwrap().1
    }
}

fn call(name: &str, arguments: Value) -> Value {
    json!({ "name": name, "arguments": arguments })
}

#[test]
fn a_fix_run_answers_the_comments_through_mcp_and_records_the_change() {
    let world = world(|work| {
        mcp_claude(
            work,
            "printf 'const count = 1;\\nexport { count };\\n' > a.ts",
            &[
                call("list_threads", json!({ "status": "open" })),
                call("reply", json!({ "threadId": "1", "body": "Renaming it." })),
                call(
                    "resolve",
                    json!({ "threadId": "1", "summary": "Fixed: renamed x to count.", "files": ["a.ts"] }),
                ),
                call("reply", json!({ "threadId": "3", "body": "Not mine." })),
                call(
                    "resolve",
                    json!({ "threadId": 2, "summary": "Fixed: renamed the export." }),
                ),
            ],
            "",
        )
    });
    let first = world.comment(1, "Rename x to count.");
    let second = world.comment(2, "Rename the export too.");
    let outside = world.comment(3, "A comment this run was not asked to fix.");
    assert_eq!((first, second, outside), (ThreadId(1), ThreadId(2), ThreadId(3)));
    let rx = world.engine.subscribe();

    let id = world
        .engine
        .submit(world.request(RunKind::FixComments, vec![first, second]))
        .unwrap();
    let (state, events) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done, "{:?}", world.engine.run(id).unwrap().error);

    let one = world.thread(first);
    let addressed = one.addressed.clone().unwrap();
    assert_eq!(
        (addressed.run, addressed.summary.as_str(), addressed.files.as_slice()),
        (id, "Fixed: renamed x to count.", ["a.ts".to_string()].as_slice())
    );
    assert_eq!(one.status, ThreadStatus::Open);
    let bodies: Vec<_> = one
        .comments
        .iter()
        .map(|comment| (comment.author.as_str(), comment.body.as_str()))
        .collect();
    assert_eq!(
        bodies,
        [
            ("You", "Rename x to count."),
            ("Claude", "Renaming it."),
            ("Claude", "Fixed: renamed x to count.")
        ]
    );
    assert_eq!(world.thread(second).addressed.unwrap().run, id);
    assert!(world.thread(outside).addressed.is_none());
    assert_eq!(
        world.thread(outside).comments.len(),
        1,
        "the out-of-scope reply was refused"
    );

    let run = world.engine.run(id).unwrap();
    assert_eq!(run.progress(), Some(Progress { done: 2, total: 2 }));
    assert!(events.iter().any(
        |event| matches!(event, RunEvent::ThreadsChanged { run, session } if *run == id && *session == world.session)
    ));
    assert_eq!(
        run.changed_files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["a.ts"]
    );

    let answers = world.mcp_out();
    assert_eq!(answers[0]["result"]["serverInfo"]["name"], "diffpilot");
    let tools: Vec<&str> = answers[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(tools, ["get_diff", "list_threads", "reply", "resolve", "dismiss"]);
    let listed: Value = serde_json::from_str(answers[2]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        listed["threads"].as_array().unwrap().len(),
        2,
        "the run sees only its own threads"
    );
    assert_eq!(answers[5]["result"]["isError"], true);

    let args = std::fs::read_to_string(out(world.work.path()).join("args.txt")).unwrap();
    assert!(args.lines().any(|arg| arg == "--mcp-config"));
    assert!(args.lines().any(|arg| arg == "--strict-mcp-config"));
    assert!(args.lines().any(|arg| arg == "mcp__diffpilot__resolve"));

    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(out(world.work.path()).join("mcp.json")).unwrap()).unwrap();
    let server = &config["mcpServers"]["diffpilot"];
    assert_eq!(server["command"], env!("CARGO_BIN_EXE_diffpilot-mcp"));
    let token = server["args"][3].as_str().unwrap();
    assert!(
        world.bridge.call(token, "list_threads", json!({})).is_err(),
        "the token dies with the run"
    );
    assert!(!world.work.path().join("runs").join(format!("run-{}", id.0)).exists());
}

#[test]
fn a_review_adds_findings_while_the_run_is_still_going() {
    let gate = |work: &Path| work.join("gate");
    let world = world(|work| {
        mcp_claude(
            work,
            "",
            &[
                call(
                    "add_comment",
                    json!({ "file": "a.ts", "startLine": 1, "body": "Name is vague. Rename it.", "title": "Vague name", "severity": "suggestion" }),
                ),
                call(
                    "add_comment",
                    json!({ "file": "a.ts", "startLine": 2, "body": "Export is unused.", "severity": "nit" }),
                ),
                call("add_general_comment", json!({ "body": "Two small things." })),
            ],
            &format!("while [ ! -e '{}' ]; do sleep 0.02; done", gate(work).display()),
        )
    });
    std::fs::write(
        world.repo.path().join("a.ts"),
        "const x = 1;\nexport { x };\nlet y = 2;\n",
    )
    .unwrap();
    let rx = world.engine.subscribe();
    let review = ReviewRequest::new(world.repo.path(), vec!["HEAD".into()], "everything");
    let request = world
        .engine
        .review_request(&review, key(world.repo.path()), "Reviewing");
    assert!(request.prompt.contains("mcp__diffpilot__add_comment"));
    assert!(request.json_schema.is_none());
    let id = world.engine.submit(request).unwrap();

    wait_for("the findings to land", WITHIN, || {
        world.store.load_threads(world.session).unwrap().value.len() == 3
    });
    assert_eq!(
        world.engine.run(id).unwrap().state,
        RunState::Running,
        "the run has not finished yet"
    );
    let threads = world.store.load_threads(world.session).unwrap().value;
    let titles: Vec<_> = threads
        .iter()
        .map(|thread| thread.finding.as_ref().map(|finding| finding.title.as_str()))
        .collect();
    assert_eq!(titles, [Some("Vague name"), Some("Export is unused."), None]);
    assert_eq!(threads[0].anchor, ThreadAnchor::line("a.ts", Side::New, 1));

    std::fs::write(gate(world.work.path()), "").unwrap();
    let (state, events) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done);
    assert_eq!(
        world.engine.run(id).unwrap().outcome,
        Some(RunOutcome::Text("All done.".into()))
    );
    let changed = events
        .iter()
        .position(|event| matches!(event, RunEvent::ThreadsChanged { .. }))
        .unwrap();
    let finished = events
        .iter()
        .position(|event| matches!(event, RunEvent::Finished { .. }))
        .unwrap();
    assert!(changed < finished);
    assert!(world.engine.run(id).unwrap().snapshot.is_none());
}

fn asking_claude(work: &Path, edit_file: &str) -> PathBuf {
    let write = format!(
        "if grep -q 'behavior[^a-z]*allow' '{}/mcp.out'; then printf 'edited\\n' > {edit_file}; fi",
        out(work).display()
    );
    mcp_claude(
        work,
        "",
        &[call(
            "approve",
            json!({ "tool_name": "Edit", "input": { "file_path": "a.ts" } }),
        )],
        &write,
    )
}

fn ask_request(world: &World, permission: EditPermission) -> RunRequest {
    let mut request = world.request(RunKind::MakeChanges, vec![]);
    request.edit_permission = permission;
    request
}

fn permission_in(events: &[RunEvent]) -> Option<diffpilot_core::mcp::PermissionRequest> {
    events.iter().find_map(|event| match event {
        RunEvent::PermissionRequest(request) => Some(request.clone()),
        _ => None,
    })
}

fn answer_next_permission(
    world: &World,
    rx: &async_channel::Receiver<RunEvent>,
    allow: bool,
) -> diffpilot_core::mcp::PermissionRequest {
    let mut seen = Vec::new();
    let deadline = std::time::Instant::now() + WITHIN;
    loop {
        assert!(std::time::Instant::now() < deadline, "no permission request: {seen:?}");
        while let Ok(event) = rx.try_recv() {
            seen.push(event);
        }
        if let Some(request) = permission_in(&seen) {
            assert!(world.engine.answer_permission(request.id, allow));
            return request;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn asking_before_an_edit_blocks_the_run_until_the_reviewer_answers() {
    let world = world(|work| asking_claude(work, "a.ts"));
    let rx = world.engine.subscribe();
    let id = world
        .engine
        .submit(ask_request(&world, EditPermission::AskEach))
        .unwrap();

    let request = answer_next_permission(&world, &rx, true);
    assert_eq!(
        (request.run, request.tool.as_str(), request.summary.as_str()),
        (id, "Edit", "Edit a.ts")
    );
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done);
    assert_eq!(
        std::fs::read_to_string(world.repo.path().join("a.ts")).unwrap(),
        "edited\n"
    );

    let args = std::fs::read_to_string(out(world.work.path()).join("args.txt")).unwrap();
    let lines: Vec<&str> = args.lines().collect();
    let at = lines.iter().position(|arg| *arg == "--permission-prompt-tool").unwrap();
    assert_eq!(lines[at + 1], "mcp__diffpilot__approve");
    assert!(!lines.contains(&"--permission-mode"));
    assert!(
        !lines.contains(&"Edit"),
        "an edit is not pre-approved when the reviewer is asked"
    );
}

#[test]
fn a_refused_edit_leaves_the_file_alone() {
    let world = world(|work| asking_claude(work, "a.ts"));
    let rx = world.engine.subscribe();
    let id = world
        .engine
        .submit(ask_request(&world, EditPermission::AskOnce))
        .unwrap();
    answer_next_permission(&world, &rx, false);
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done);
    assert_eq!(
        std::fs::read_to_string(world.repo.path().join("a.ts")).unwrap(),
        "const x = 1;\nexport { x };\n"
    );
    let answers = world.mcp_out();
    assert!(
        answers.last().unwrap()["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("deny")
    );
}

#[test]
fn stopping_a_run_that_waits_for_permission_ends_it() {
    let world = world(|work| asking_claude(work, "a.ts"));
    let rx = world.engine.subscribe();
    let id = world
        .engine
        .submit(ask_request(&world, EditPermission::AskEach))
        .unwrap();
    let mut seen = Vec::new();
    wait_for("a request", WITHIN, || {
        while let Ok(event) = rx.try_recv() {
            seen.push(event);
        }
        permission_in(&seen).is_some()
    });
    assert!(world.engine.stop(id));
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Stopped);
    assert!(!world.engine.answer_permission(permission_in(&seen).unwrap().id, true));
}

#[test]
fn a_planned_fix_run_fills_in_the_files_that_claude_did_not_name() {
    let world = world(|work| {
        mcp_claude(
            work,
            "printf 'const count = 1;\\nexport { count };\\n' > a.ts",
            &[
                call(
                    "resolve",
                    json!({ "threadId": "1", "summary": "Fixed: renamed x to count." }),
                ),
                call(
                    "resolve",
                    json!({ "threadId": "2", "summary": "Fixed: nothing to change." }),
                ),
            ],
            "",
        )
    });
    let first = world.comment_in("a.ts", 1, "Rename x to count.");
    let second = world.comment_in("b.ts", 1, "Export it as default.");
    let closed = world.comment_in("a.ts", 2, "Old note.");
    world.store.set_status(closed, ThreadStatus::Resolved).unwrap();
    let plan = fix::fix_comments(&world.target(EditPermission::DontAsk), &world.threads(), &[]).unwrap();
    assert_eq!(plan.skipped, [(closed, Skip::Resolved)]);
    let rx = world.engine.subscribe();

    let id = world.engine.submit(plan.request).unwrap();
    let (state, events) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done, "{:?}", world.engine.run(id).unwrap().error);

    let run = world.engine.run(id).unwrap();
    assert_eq!(run.threads, [first, second]);
    assert_eq!(run.progress(), Some(Progress { done: 2, total: 2 }));
    let one = world.thread(first).addressed.unwrap();
    assert_eq!((one.run, one.files.as_slice()), (id, ["a.ts".to_string()].as_slice()));
    assert!(
        world.thread(second).addressed.unwrap().files.is_empty(),
        "b.ts did not change"
    );
    assert!(world.thread(closed).addressed.is_none());
    assert_eq!(
        run.changed_files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["a.ts"]
    );

    let last_change = events
        .iter()
        .rposition(|event| matches!(event, RunEvent::ThreadsChanged { .. }))
        .unwrap();
    let finished = events
        .iter()
        .position(|event| matches!(event, RunEvent::Finished { .. }))
        .unwrap();
    assert!(
        last_change < finished,
        "the thread files are known before the run is reported finished"
    );

    let args = std::fs::read_to_string(out(world.work.path()).join("args.txt")).unwrap();
    assert!(args.contains("- Thread `1`: `a.ts:1`"));
    assert!(args.contains("- Thread `2`: `b.ts:1`"));
    assert!(!args.contains("Thread `3`"));
}

#[test]
fn after_a_refused_edit_claude_cannot_resolve_the_thread() {
    let world = world(|work| {
        mcp_claude(
            work,
            "",
            &[
                call(
                    "approve",
                    json!({ "tool_name": "Edit", "input": { "file_path": "a.ts" } }),
                ),
                call("resolve", json!({ "threadId": "1", "summary": "Fixed: renamed x." })),
                call(
                    "reply",
                    json!({ "threadId": "1", "body": "You refused the edit. I would rename x to count." }),
                ),
            ],
            "",
        )
    });
    let thread = world.comment(1, "Rename x to count.");
    let plan = fix::fix_comments(&world.target(EditPermission::AskEach), &world.threads(), &[]).unwrap();
    let rx = world.engine.subscribe();
    let id = world.engine.submit(plan.request).unwrap();
    answer_next_permission(&world, &rx, false);
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done);

    let answers = world.mcp_out();
    assert_eq!(answers[3]["result"]["isError"], true);
    assert!(
        answers[3]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("edit_rejected")
    );
    assert!(world.thread(thread).addressed.is_none());
    let comments = world.thread(thread).comments;
    assert_eq!(comments.len(), 2);
    assert!(comments[1].body.contains("refused"));
    assert_eq!(
        world.engine.run(id).unwrap().progress(),
        Some(Progress { done: 1, total: 1 }),
        "a reply counts as an answer"
    );
    assert!(world.engine.run(id).unwrap().changed_files.is_empty());
}

#[test]
fn a_reply_run_answers_one_thread_without_marking_it_fixed() {
    let world = world(|work| {
        mcp_claude(
            work,
            "",
            &[call(
                "reply",
                json!({ "threadId": "1", "body": "It keeps the old name for callers." }),
            )],
            "",
        )
    });
    let thread = world.comment(1, "Why is x not renamed?");
    let target = world.target(EditPermission::DontAsk);
    let request = fix::reply_to(&target, &world.threads()[0]).unwrap();
    let rx = world.engine.subscribe();
    let id = world.engine.submit(request).unwrap();
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done);
    assert_eq!(world.thread(thread).comments[1].author, "Claude");
    assert!(world.thread(thread).addressed.is_none(), "an answer is not a fix");
    assert_eq!(world.engine.run(id).unwrap().kind, RunKind::Reply);
}

#[test]
fn a_change_run_edits_files_with_no_thread_and_can_be_undone() {
    let world = world(|work| mcp_claude(work, "printf 'export const y = 1;\\n' > c.ts", &[], ""));
    let request = fix::make_changes(&world.target(EditPermission::DontAsk), "Add c.ts that exports y.").unwrap();
    let rx = world.engine.subscribe();
    let id = world.engine.submit(request).unwrap();
    let (state, _) = wait_finished(&rx, id, WITHIN);
    assert_eq!(state, RunState::Done);

    let run = world.engine.run(id).unwrap();
    assert!(run.threads.is_empty());
    assert_eq!(
        run.changed_files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["c.ts"]
    );
    let args = std::fs::read_to_string(out(world.work.path()).join("args.txt")).unwrap();
    assert!(args.contains("> Add c.ts that exports y."));
    assert!(args.lines().any(|arg| arg == "mcp__diffpilot__get_diff"));
    assert!(!args.lines().any(|arg| arg == "mcp__diffpilot__resolve"));

    let report = world.engine.undo_run(id, false).unwrap();
    assert!(report.conflicts.is_empty());
    assert!(!world.repo.path().join("c.ts").exists());
}
