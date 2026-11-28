mod common;

use common::*;
use diffpilot_core::diff;
use diffpilot_core::guide::{self, Attention, DiagramKind, Graph, StopKind, prompt};
use diffpilot_core::mcp::{Binding, McpBridge, Notice};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::review::{Effort, ModelName, ReviewModel};
use diffpilot_core::run::{LogKind, RunId, RunKind, RunOutcome, RunState};
use diffpilot_core::runner::{EngineError, McpLaunch, RunEngine, RunEvent};
use diffpilot_core::store::{SessionId, Store};
use diffpilot_core::threads::Side;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const WITHIN: Duration = Duration::from_secs(30);

fn change_repo() -> tempfile::TempDir {
    let repo = temp_repo();
    std::fs::write(
        repo.path().join("a.ts"),
        "const count = 1;\nexport { count };\nexport const extra = 3;\n",
    )
    .unwrap();
    std::fs::write(repo.path().join("c.ts"), "export const fresh = 1;\n").unwrap();
    git(repo.path(), &["add", "-A"]);
    repo
}

fn call(name: &str, arguments: Value) -> Value {
    json!({ "name": name, "arguments": arguments })
}

fn overview() -> Value {
    call(
        "set_overview",
        json!({
            "title": "Counting is clearer",
            "summary": "The counter has a real name, and a new module holds a fresh value.",
            "before": "A bare x.",
            "after": "A named count."
        }),
    )
}

fn chapter_a() -> Value {
    call(
        "add_chapter",
        json!({
            "title": "Rename the counter",
            "summary": "A reader can tell what the number counts.",
            "phase": "Core",
            "attention": "high",
            "points": ["The export keeps working."],
            "files": ["a.ts"],
            "impact": [{ "site": "b.ts consumers", "path": "b.ts", "line": 1, "text": "They import the old name." }]
        }),
    )
}

fn note_a() -> Value {
    call(
        "add_note",
        json!({ "chapter": 1, "file": "a.ts", "line": 3, "text": "An extra export nobody asked for.", "critical": true, "kind": "read", "check": "Nothing imports it.", "node": "Count" }),
    )
}

fn diagram_a() -> Value {
    call(
        "add_diagram",
        json!({
            "chapter": 1,
            "title": "Who calls what",
            "source": "flowchart LR\n  Caller[Caller] --> Count[count]:::changed\n  %% ref node:Count [[a.ts#L1]]"
        }),
    )
}

fn chapter_c() -> Value {
    call(
        "add_chapter",
        json!({ "title": "A fresh value", "summary": "A new module.", "attention": "low", "files": ["c.ts"] }),
    )
}

fn finish() -> Value {
    call("finish_guide", json!({}))
}

fn full_guide() -> Vec<Value> {
    vec![overview(), chapter_a(), note_a(), diagram_a(), chapter_c(), finish()]
}

/// The assistant lines a real `claude -p --output-format stream-json` prints for these calls.
fn recorded(calls: &[Value]) -> String {
    calls
        .iter()
        .enumerate()
        .map(|(index, call)| {
            let line = json!({
                "type": "assistant",
                "message": {
                    "id": format!("msg_{index}"),
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": format!("toolu_{index}"),
                        "name": format!("mcp__diffpilot__{}", call["name"].as_str().unwrap()),
                        "input": call["arguments"],
                    }]
                }
            });
            format!("printf '%s\\n' '{line}'\n")
        })
        .collect()
}

struct World {
    repo: tempfile::TempDir,
    work: tempfile::TempDir,
    store: Arc<Store>,
    session: SessionId,
    engine: RunEngine,
}

fn world(calls: &[Value]) -> World {
    let repo = change_repo();
    let work = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = store.session(&key(repo.path())).unwrap();
    let bridge = McpBridge::start(work.path().join("mcp.sock"), store.clone()).unwrap();
    let claude = mcp_claude(work.path(), &recorded(calls), calls, "");
    let mut config = config(store.clone(), claude, work.path());
    let mut mcp = McpLaunch::new(bridge);
    mcp.binary = bin("diffpilot-mcp");
    config.mcp = Some(mcp);
    World {
        engine: RunEngine::new(config),
        repo,
        work,
        store,
        session,
    }
}

impl World {
    fn target(&self) -> diffpilot_core::fix::Target {
        diffpilot_core::fix::Target {
            repo: self.repo.path().to_path_buf(),
            session: key(self.repo.path()),
            revisions: vec!["HEAD".into()],
            model: ReviewModel::STANDARD,
            edit_permission: EditPermission::DontAsk,
        }
    }

    fn write_guide(&self) -> (RunId, RunState, Vec<RunEvent>) {
        let rx = self.engine.subscribe();
        let id = self.engine.submit(prompt::request(&self.target(), "", &[])).unwrap();
        let (state, events) = wait_finished(&rx, id, WITHIN);
        (id, state, events)
    }

    fn mcp_out(&self) -> Vec<Value> {
        std::fs::read_to_string(out(self.work.path()).join("mcp.out"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

#[test]
fn a_guide_run_builds_the_guide_through_mcp_and_saves_it_for_the_review() {
    let world = world(&full_guide());
    let (id, state, events) = world.write_guide();
    let run = world.engine.run(id).unwrap();
    assert_eq!(state, RunState::Done, "{:?}", run.error);

    let stored = world.store.current_guide(world.session).unwrap().unwrap();
    assert_eq!(run.outcome, Some(RunOutcome::Guide(stored.id)));
    assert_eq!((stored.run, stored.model), (Some(id), Some(ReviewModel::STANDARD)));
    let guide = &stored.guide;
    assert_eq!(guide.overview.title, "Counting is clearer");
    assert_eq!(guide.overview.before.as_deref(), Some("A bare x."));
    let titles: Vec<_> = guide.chapters.iter().map(|chapter| chapter.title.as_str()).collect();
    assert_eq!(titles, ["Rename the counter", "A fresh value"]);
    let first = &guide.chapters[0];
    assert_eq!(
        (first.attention, first.phase.as_deref()),
        (Attention::High, Some("Core"))
    );
    assert_eq!(first.impact[0].path, "b.ts");
    let note = &first.notes[0];
    let span = note.span.as_ref().unwrap();
    assert_eq!((note.critical, span.side, span.line), (true, Side::New, 3));
    assert_eq!(span.anchor_text.as_deref(), Some("export const extra = 3;"));
    assert_eq!(first.diagrams[0].kind, DiagramKind::Flowchart);
    let Graph::Flowchart(chart) = first.diagrams[0].graph().unwrap() else {
        panic!("a flowchart");
    };
    assert_eq!((chart.nodes.len(), chart.edges.len(), chart.refs.len()), (2, 1, 1));
    assert_eq!(
        (note.kind, note.check.as_deref(), note.node.as_deref()),
        (StopKind::Read, Some("Nothing imports it."), Some("Count"))
    );
    assert_eq!(guide.chapters[1].attention, Attention::Low);
    assert!(guide.head.is_some() && !guide.fingerprint.is_empty());

    let files = diff::load_revisions(world.repo.path(), &["HEAD".to_string()]).unwrap();
    let status = guide::check(guide, &files);
    assert!(!status.is_stale && status.uncovered_files.is_empty());
    assert!(status.chapters.iter().all(|chapter| chapter.is_current()));

    assert!(events.iter().any(
        |event| matches!(event, RunEvent::GuideChanged { run, session } if *run == id && *session == world.session)
    ));
    let lines: Vec<_> = run
        .log
        .iter()
        .filter(|entry| entry.kind == LogKind::Comment)
        .map(|entry| entry.text.as_str())
        .collect();
    assert_eq!(lines[1], "Added chapter: Rename the counter");
    assert_eq!(lines.last(), Some(&"Saved the guide"));

    let answers = world.mcp_out();
    let tools: Vec<&str> = answers[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tools,
        [
            "get_diff",
            "set_overview",
            "add_chapter",
            "add_note",
            "add_diagram",
            "finish_guide"
        ]
    );
    assert!(
        answers[2..].iter().all(|answer| answer["result"]["isError"] == false),
        "{answers:?}"
    );
    let chapter_answer: Value =
        serde_json::from_str(answers[3]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(chapter_answer["chapter"], 1);

    let args = std::fs::read_to_string(out(world.work.path()).join("args.txt")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    let after = |flag: &str| args[args.iter().position(|arg| *arg == flag).unwrap() + 1];
    assert_eq!((after("--model"), after("--effort")), ("opus", "high"));
    assert!(args.contains(&"mcp__diffpilot__finish_guide") && args.contains(&"Bash(git push:*)"));
    assert!(!args.contains(&"Edit") && !args.contains(&"Write") && !args.contains(&"--permission-mode"));
    assert!(args[args.iter().position(|arg| *arg == "-p").unwrap() + 1].contains("# Write a reading guide"));
    assert!(
        run.snapshot.is_none(),
        "a guide run is read-only, so it takes no snapshot"
    );
}

#[test]
fn a_guide_goes_stale_when_the_diff_moves_and_a_new_run_makes_the_old_one_a_past_guide() {
    let world = world(&full_guide());
    let (first, state, _) = world.write_guide();
    assert_eq!(state, RunState::Done);
    let guide = world.store.current_guide(world.session).unwrap().unwrap().guide;

    std::fs::write(world.repo.path().join("c.ts"), "export const fresh = 2;\n").unwrap();
    git(world.repo.path(), &["add", "-A"]);
    let files = diff::load_revisions(world.repo.path(), &["HEAD".to_string()]).unwrap();
    let status = guide::check(&guide, &files);
    assert!(status.is_stale);
    assert_eq!(status.chapters[1].changed_files, ["c.ts"]);
    assert!(status.chapters[0].is_current());

    let (second, state, _) = world.write_guide();
    assert_eq!(state, RunState::Done);
    assert_ne!(first, second);
    let listed = world.store.guides(world.session, 10).unwrap();
    assert_eq!(
        listed.iter().map(|row| row.run).collect::<Vec<_>>(),
        [Some(second), Some(first)]
    );
    let current = world.store.current_guide(world.session).unwrap().unwrap();
    assert!(!guide::check(&current.guide, &files).is_stale);
    assert_eq!(listed[1].chapters, 2);
}

#[test]
fn a_guide_claude_did_not_finish_is_finished_by_the_app_and_files_left_out_get_a_chapter() {
    let world = world(&[overview(), chapter_a()]);
    let (id, state, _) = world.write_guide();
    assert_eq!(state, RunState::Done, "{:?}", world.engine.run(id).unwrap().error);
    let guide = world.store.current_guide(world.session).unwrap().unwrap().guide;
    let titles: Vec<_> = guide.chapters.iter().map(|chapter| chapter.title.as_str()).collect();
    assert_eq!(titles, ["Rename the counter", "Everything else"]);
    assert_eq!(guide.chapters[1].files[0].path, "c.ts");
}

#[test]
fn a_run_that_wrote_nothing_fails_and_leaves_no_guide_or_draft() {
    let world = world(&[]);
    let (id, state, _) = world.write_guide();
    assert_eq!(state, RunState::Failed);
    assert_eq!(
        world.engine.run(id).unwrap().error.as_deref(),
        Some("Claude did not write a guide.")
    );
    assert!(world.store.current_guide(world.session).unwrap().is_none());
    assert!(world.store.load_draft(id).unwrap().is_none());

    let overview_only = self::world(&[overview()]);
    let (id, state, _) = overview_only.write_guide();
    assert_eq!(state, RunState::Failed);
    assert!(
        overview_only
            .engine
            .run(id)
            .unwrap()
            .error
            .unwrap()
            .contains("Claude did not finish the guide")
    );
    assert!(
        overview_only.store.load_draft(id).unwrap().is_none(),
        "a failed run keeps no draft"
    );
}

#[test]
fn a_mistake_comes_back_as_an_error_on_that_call_and_the_rest_of_the_guide_stays() {
    let too_long = call(
        "add_chapter",
        json!({ "title": "Long", "summary": "word ".repeat(60), "files": ["c.ts"] }),
    );
    let outside = call(
        "add_chapter",
        json!({ "title": "Gone", "summary": "S.", "files": ["nope.ts"] }),
    );
    let bad_diagram = call("add_diagram", json!({ "source": "flowchart LR\n  A --> B\n  B -->" }));
    let line_outside = call(
        "add_note",
        json!({ "chapter": 1, "file": "a.ts", "line": 60, "text": "Nope." }),
    );
    let mut calls = vec![overview(), chapter_a(), too_long, outside, bad_diagram, line_outside];
    calls.extend([chapter_c(), finish()]);
    let world = world(&calls);
    let (id, state, _) = world.write_guide();
    assert_eq!(state, RunState::Done, "{:?}", world.engine.run(id).unwrap().error);

    let answers = world.mcp_out();
    let result = |at: usize| answers[at]["result"].clone();
    let text = |at: usize| result(at)["content"][0]["text"].as_str().unwrap().to_string();
    assert_eq!(result(4)["isError"], true);
    assert!(text(4).contains("summary is 60 words (max 40)"), "{}", text(4));
    assert!(
        text(5).contains("not in the diff") && text(5).contains("a.ts"),
        "{}",
        text(5)
    );
    assert!(text(6).contains("Mermaid line 3"), "{}", text(6));
    assert!(text(7).contains("not in the diff on the new side"), "{}", text(7));

    let guide = world.store.current_guide(world.session).unwrap().unwrap().guide;
    let titles: Vec<_> = guide.chapters.iter().map(|chapter| chapter.title.as_str()).collect();
    assert_eq!(titles, ["Rename the counter", "A fresh value"]);
    assert!(guide.chapters[0].notes.is_empty() && guide.diagrams.is_empty());
}

#[test]
fn the_guide_tools_belong_to_a_guide_run_and_stop_after_the_guide_is_saved() {
    let repo = change_repo();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let session = store.session(&key(repo.path())).unwrap();
    let socket = tempfile::tempdir().unwrap();
    let bridge = McpBridge::start(socket.path().join("mcp.sock"), store.clone()).unwrap();
    let notices = Arc::new(Mutex::new(Vec::new()));
    let token = |kind: RunKind, run: i64| {
        let notices = notices.clone();
        bridge.register(Binding {
            run: RunId(run),
            session,
            repo: repo.path().to_path_buf(),
            kind,
            revisions: vec!["HEAD".into()],
            linked: Vec::new(),
            threads: Vec::new(),
            edits: Some(EditPermission::DontAsk),
            notify: Arc::new(move |notice| notices.lock().unwrap().push(notice)),
        })
    };
    let ask = |token: &str, name: &str, args: Value| bridge.call(token, name, args).map_err(|error| error.0);

    let review = token(RunKind::Review, 1);
    let refused = ask(&review, "set_overview", json!({ "title": "T", "summary": "S." })).unwrap_err();
    assert!(refused.contains("not available in a review run"), "{refused}");

    let guide = token(RunKind::Guide, 2);
    let refused = ask(
        &guide,
        "add_comment",
        json!({ "file": "a.ts", "startLine": 1, "body": "x" }),
    )
    .unwrap_err();
    assert!(refused.contains("not available in a guide run"), "{refused}");
    assert!(
        ask(&guide, "finish_guide", json!({}))
            .unwrap_err()
            .contains("nothing to save")
    );
    assert!(
        ask(&guide, "add_note", json!({ "chapter": 1, "file": "a.ts", "text": "x" }))
            .unwrap_err()
            .contains("no chapter 1")
    );
    assert!(
        ask(&guide, "add_chapter", json!({ "title": "x" }))
            .unwrap_err()
            .starts_with("Bad arguments")
    );

    ask(
        &guide,
        "set_overview",
        json!({ "title": "Title", "summary": "Summary." }),
    )
    .unwrap();
    assert!(
        store.current_guide(session).unwrap().is_none(),
        "a draft is not a guide yet"
    );
    let draft = store.load_draft(RunId(2)).unwrap().unwrap();
    assert_eq!(draft.guide.overview.summary, "Summary.");
    assert_eq!(
        draft.model,
        Some(ReviewModel::STANDARD),
        "no run row, so the standard model is recorded"
    );

    let range = json!({ "path": "a.ts", "ranges": [{ "startLine": 1, "endLine": 2 }] });
    let tail = json!({ "path": "a.ts", "ranges": [{ "startLine": 3 }] });
    assert_eq!(
        ask(
            &guide,
            "add_chapter",
            json!({ "title": "Head", "summary": "S.", "files": [range] })
        )
        .unwrap()["chapter"],
        1
    );
    let unknown = ask(
        &guide,
        "add_chapter",
        json!({ "title": "Tail", "summary": "S.", "files": [tail, "c.ts", "b.ts"] }),
    )
    .unwrap_err();
    assert!(unknown.contains("b.ts"), "{unknown}");
    assert_eq!(
        ask(
            &guide,
            "add_chapter",
            json!({ "title": "Tail", "summary": "S.", "files": [tail, "c.ts"] })
        )
        .unwrap()["chapter"],
        2
    );

    let done = ask(&guide, "finish_guide", json!({})).unwrap();
    assert_eq!(done["saved"], true);
    assert_eq!(done["chapters"], 2);
    assert_eq!(done["filesAddedToEverythingElse"], json!([]));
    assert!(store.current_guide(session).unwrap().is_some());
    assert!(
        ask(&guide, "add_note", json!({ "chapter": 1, "file": "a.ts", "text": "x" }))
            .unwrap_err()
            .contains("guide is saved")
    );
    assert!(
        ask(&guide, "finish_guide", json!({}))
            .unwrap_err()
            .contains("guide is saved")
    );
    assert!(
        notices
            .lock()
            .unwrap()
            .iter()
            .all(|notice| *notice == Notice::GuideChanged)
    );
    assert_eq!(notices.lock().unwrap().len(), 4);
}

#[test]
fn a_guide_run_needs_the_mcp_server() {
    let repo = change_repo();
    let work = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let engine = RunEngine::new(config(store, fake_claude(work.path(), "exit 0"), work.path()));
    let target = diffpilot_core::fix::Target {
        repo: repo.path().to_path_buf(),
        session: key(repo.path()),
        revisions: vec!["HEAD".into()],
        model: ReviewModel::new(ModelName::Sonnet, Effort::Low),
        edit_permission: EditPermission::DontAsk,
    };
    assert!(matches!(
        engine.submit(prompt::request(&target, "", &[])),
        Err(EngineError::GuideNeedsMcp)
    ));
}
