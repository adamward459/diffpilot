use diffpilot::diff_pane::rows::{BlockId, RowKind};
use diffpilot::guide::session::{GuideRun, Step};
use diffpilot::repo_view::{Nav, RepoView};
use diffpilot::runs::Runs;
use diffpilot::testing::{canned_claude, demo_guide, fixture, open_repo_in};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::FileDiff;
use diffpilot_core::guide::{Diagram, DiagramKind, FlowStep, Guide};
use diffpilot_core::mcp::McpBridge;
use diffpilot_core::review::ReviewModel;
use diffpilot_core::run::RunKind;
use diffpilot_core::runner::{EngineConfig, McpLaunch, RunEngine};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::{RunId, Store};
use diffpilot_core::viewed::ViewedStore;
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct World {
    view: Entity<RepoView>,
    store: Arc<Store>,
    repo: tempfile::TempDir,
    work: tempfile::TempDir,
}

fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn gated_claude(work: &Path) -> std::path::PathBuf {
    let out = work.join("out");
    std::fs::create_dir_all(&out).unwrap();
    script(
        work,
        "claude",
        &format!(
            r#"
echo '{{"type":"system","subtype":"init","session_id":"s-1","model":"m","tools":[],"mcp_servers":[]}}'
touch '{out}/started'
while [ ! -f '{out}/go' ]; do sleep 0.05; done
echo '{{"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"Done."}}'
"#,
            out = out.display()
        ),
    )
}

fn open(cx: &mut TestAppContext, with_runs: bool) -> (World, &'static mut VisualTestContext) {
    let repo = fixture();
    let work = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    if with_runs {
        let guard = script(
            work.path(),
            "guard",
            "cat > /dev/null\necho '[diffpilot guard] blocked' >&2\nexit 2",
        );
        let bridge = McpBridge::start(work.path().join("mcp.sock"), store.clone()).unwrap();
        let mut config = EngineConfig::new(store.clone());
        config.claude_binary = Some(gated_claude(work.path()));
        config.guard_binary = guard;
        config.run_dir = work.path().join("runs");
        config.keep_snapshots = 0;
        config.mcp = Some(McpLaunch::new(bridge));
        let engine = RunEngine::new(config);
        cx.update(|cx| cx.set_global(Runs::with_engine(engine)));
    }
    let (view, vcx) = open_repo_in(
        cx,
        repo.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
        store.clone(),
        canned_claude(),
    );
    (
        World {
            view,
            store,
            repo,
            work,
        },
        vcx,
    )
}

fn diff_files(world: &World, cx: &mut VisualTestContext) -> Vec<FileDiff> {
    let review = world.view.read_with(cx, |view, _| view.review());
    review.read_with(cx, |review, _| {
        review.host().files.iter().map(|file| (**file).clone()).collect()
    })
}

fn store_guide(world: &World, cx: &mut VisualTestContext) {
    store_guide_with(world, cx, |_| {});
}

fn store_guide_with(world: &World, cx: &mut VisualTestContext, edit: impl FnOnce(&mut Guide)) {
    let files = diff_files(world, cx);
    let mut guide = demo_guide(world.repo.path(), &files);
    edit(&mut guide);
    let (review, guide_session) = world.view.read_with(cx, |view, _| (view.review(), view.guide()));
    let session = review.read_with(cx, |review, _| review.session_id());
    world
        .store
        .save_draft(session, RunId(1), ReviewModel::STANDARD, &guide)
        .unwrap();
    world.store.complete_draft(RunId(1), &guide).unwrap();
    guide_session.update(cx, |guide, cx| guide.reload(cx));
    cx.run_until_parked();
}

fn show_guide(world: &World, cx: &mut VisualTestContext) {
    world
        .view
        .update_in(cx, |view, window, cx| view.show_nav(Nav::Guide, window, cx));
    cx.run_until_parked();
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not on screen"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

fn pane_paths(world: &World, cx: &mut VisualTestContext) -> Vec<String> {
    let pane = world.view.read_with(cx, |view, _| view.pane());
    pane.read_with(cx, |pane, _| {
        pane.state().files.iter().map(|file| file.path.clone()).collect()
    })
}

fn wait_until(cx: &mut VisualTestContext, what: &str, mut done: impl FnMut(&mut VisualTestContext) -> bool) {
    cx.executor().allow_parking();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[gpui::test]
fn the_guide_switch_without_a_guide_shows_the_start_card(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    show_guide(&world, cx);
    assert_eq!(world.view.read_with(cx, |view, _| view.nav()), Nav::Guide);
    assert!(cx.debug_bounds("guide-write").is_some(), "the Write Guide button");
    assert_eq!(pane_paths(&world, cx).len(), 3, "the file pane is untouched");

    world
        .view
        .update_in(cx, |view, window, cx| view.show_nav(Nav::Files, window, cx));
    cx.run_until_parked();
    assert_eq!(world.view.read_with(cx, |view, _| view.nav()), Nav::Files);
}

#[gpui::test]
fn write_guide_submits_a_guide_run_and_reports_a_run_that_wrote_nothing(app: &mut TestAppContext) {
    let (world, cx) = open(app, true);
    show_guide(&world, cx);
    click(cx, "guide-write");

    let guide = world.view.read_with(cx, |view, _| view.guide());
    let GuideRun::Writing { run, .. } = guide.read_with(cx, |guide, _| guide.run().clone()) else {
        panic!("a run is writing");
    };
    let kind = cx.update(|_, cx| cx.global::<Runs>().engine().run(run).map(|run| run.kind));
    assert_eq!(kind, Some(RunKind::Guide));
    assert!(
        cx.debug_bounds("guide-stop").is_some(),
        "the Stop button shows while writing"
    );

    std::fs::write(world.work.path().join("out/go"), "").unwrap();
    wait_until(cx, "the run to end", |cx| {
        !matches!(
            guide.read_with(cx, |guide, _| guide.run().clone()),
            GuideRun::Writing { .. }
        )
    });
    let run = guide.read_with(cx, |guide, _| guide.run().clone());
    assert!(
        matches!(&run, GuideRun::Failed(message) if !message.is_empty()),
        "{run:?}"
    );
    assert!(cx.debug_bounds("guide-write").is_some(), "Try Again is offered");
}

#[gpui::test]
fn a_stored_guide_opens_on_the_overview_and_start_reading_shows_chapter_one(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide(&world, cx);
    show_guide(&world, cx);
    let guide = world.view.read_with(cx, |view, _| view.guide());
    assert_eq!(guide.read_with(cx, |guide, _| guide.step()), Step::Overview);
    assert!(cx.debug_bounds("guide-start-reading").is_some());
    assert_eq!(pane_paths(&world, cx).len(), 3);

    click(cx, "guide-start-reading");
    assert_eq!(guide.read_with(cx, |guide, _| guide.step()), Step::Chapter(0));
    assert_eq!(pane_paths(&world, cx), ["src/staged.rs"]);

    let pane = world.view.read_with(cx, |view, _| view.pane());
    pane.read_with(cx, |pane, _| {
        let state = pane.state();
        assert_eq!(state.blocks.len(), 2);
        for (slot, line) in [(0u32, 5u32), (1, 7)] {
            assert_eq!(state.blocks[slot as usize].id, BlockId::Note(slot));
            let row = state.rows.block_row(slot).expect("the note is placed");
            let RowKind::Line { hunk, line: at } = state.rows.rows[row - 1].kind else {
                panic!("a note sits under a line");
            };
            assert_eq!(state.files[0].hunks[hunk as usize].lines[at as usize].new, Some(line));
        }
    });
    assert!(
        cx.debug_bounds("guide-mark-continue").is_some(),
        "the inspector shows in a chapter"
    );
}

#[gpui::test]
fn mark_viewed_and_continue_marks_the_chapter_and_opens_the_next(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide(&world, cx);
    show_guide(&world, cx);
    click(cx, "guide-start-reading");
    click(cx, "guide-mark-continue");
    cx.run_until_parked();

    let guide = world.view.read_with(cx, |view, _| view.guide());
    assert_eq!(guide.read_with(cx, |guide, _| guide.step()), Step::Chapter(1));
    assert_eq!(pane_paths(&world, cx), ["base.txt", "docs/committed.md"]);
    let viewed = guide.read_with(cx, |guide, _| guide.viewed().clone());
    assert!(viewed.contains("src/staged.rs"), "{viewed:?}");
    assert_eq!(guide.read_with(cx, |guide, _| guide.read_count()), 1);
    assert_eq!(guide.read_with(cx, |guide, _| guide.first_unread()), 1);
}

#[gpui::test]
fn a_changed_diff_shows_the_out_of_date_banner_until_the_reader_keeps_reading(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide(&world, cx);
    show_guide(&world, cx);
    assert!(cx.debug_bounds("guide-out-of-date").is_none());

    let mut files = diff_files(&world, cx);
    let changed = files.iter_mut().find(|file| file.path == "base.txt").unwrap();
    changed.hunks[0].lines[0].text.push_str(" edited");
    let pane = world.view.read_with(cx, |view, _| view.pane());
    pane.update(cx, |pane, cx| pane.set_files(files, cx));
    cx.run_until_parked();

    let guide = world.view.read_with(cx, |view, _| view.guide());
    assert!(guide.read_with(cx, |guide, _| guide.out_of_date()));
    assert!(cx.debug_bounds("guide-out-of-date").is_some());

    click(cx, "guide-keep-reading");
    assert!(!guide.read_with(cx, |guide, _| guide.out_of_date()), "kept");
}

#[gpui::test]
fn the_chapter_keys_step_through_the_chapters_and_back_to_the_overview(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide(&world, cx);
    show_guide(&world, cx);
    click(cx, "guide-start-reading");
    let guide = world.view.read_with(cx, |view, _| view.guide());
    let step = |cx: &mut VisualTestContext| guide.read_with(cx, |guide, _| guide.step());

    world.view.update(cx, |view, cx| view.step_chapter(1, cx));
    assert_eq!(step(cx), Step::Chapter(1));
    world.view.update(cx, |view, cx| view.step_chapter(1, cx));
    assert_eq!(step(cx), Step::Overview, "past the last chapter is the overview");

    world.view.update(cx, |view, cx| view.step_chapter(-1, cx));
    assert_eq!(step(cx), Step::Overview, "the keys do nothing outside a chapter");
}

fn flowchart() -> Diagram {
    Diagram {
        title: Some("Undo path".into()),
        kind: DiagramKind::Flowchart,
        source: "flowchart LR\n  A[Browser]:::changed -->|POST| B{Valid}\n  B --> C(Handler):::added".into(),
        flow: vec![
            FlowStep {
                node: "A".into(),
                text: "The browser sends the request.".into(),
            },
            FlowStep {
                node: "B".into(),
                text: "The request is checked.".into(),
            },
        ],
    }
}

#[gpui::test]
fn a_flowchart_is_drawn_on_the_overview_and_the_source_switch_shows_its_text(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide_with(&world, cx, |guide| guide.diagrams = vec![flowchart()]);
    show_guide(&world, cx);

    let canvas = cx.debug_bounds("guide-diagram-canvas").expect("the diagram is drawn");
    assert!(
        canvas.size.width > gpui::px(50.) && canvas.size.height > gpui::px(20.),
        "{canvas:?}"
    );
    assert!(cx.debug_bounds("guide-diagram-source").is_none());

    click(cx, "guide-diagram-view:Source");
    assert!(cx.debug_bounds("guide-diagram-source").is_some());
}

#[gpui::test]
fn the_walkthrough_shows_the_step_text_under_the_diagram(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide_with(&world, cx, |guide| guide.diagrams = vec![flowchart()]);
    show_guide(&world, cx);
    assert!(cx.debug_bounds("guide-diagram-step-text").is_none());

    click(cx, "guide-diagram-next");
    assert!(cx.debug_bounds("guide-diagram-step-text").is_some());
}

#[gpui::test]
fn a_diagram_that_cannot_be_drawn_falls_back_to_its_source_with_a_note(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide_with(&world, cx, |guide| {
        guide.diagrams = vec![Diagram {
            title: None,
            kind: DiagramKind::Other("gantt".into()),
            source: "gantt\n  title Plan".into(),
            flow: vec![],
        }];
    });
    show_guide(&world, cx);
    assert!(cx.debug_bounds("guide-diagram-canvas").is_none());
    assert!(cx.debug_bounds("guide-diagram-note").is_some());
    assert!(cx.debug_bounds("guide-diagram-source").is_some());
    assert!(
        cx.debug_bounds("guide-diagram-view:Source").is_none(),
        "nothing to switch to"
    );
}

#[gpui::test]
fn a_chapter_diagram_is_drawn_in_the_inspector(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide_with(&world, cx, |guide| {
        guide.chapters[0].diagrams = vec![Diagram {
            title: Some("Retry".into()),
            kind: DiagramKind::Sequence,
            source: "sequenceDiagram\n  A->>B: request\n  B-->>A: reply".into(),
            flow: vec![],
        }];
    });
    show_guide(&world, cx);
    click(cx, "guide-start-reading");
    assert!(cx.debug_bounds("guide-diagram-canvas").is_some());
}

#[gpui::test]
fn a_second_write_while_writing_leaves_the_live_run_alone(app: &mut TestAppContext) {
    let (world, cx) = open(app, true);
    show_guide(&world, cx);
    click(cx, "guide-write");
    let guide = world.view.read_with(cx, |view, _| view.guide());
    let before = guide.read_with(cx, |guide, _| guide.run().clone());
    assert!(matches!(before, GuideRun::Writing { .. }));

    guide.update(cx, |guide, cx| guide.write(cx));
    assert_eq!(guide.read_with(cx, |guide, _| guide.run().clone()), before);
}

#[gpui::test]
fn a_rebuilt_session_adopts_the_run_that_is_still_writing(app: &mut TestAppContext) {
    let (world, cx) = open(app, true);
    show_guide(&world, cx);
    click(cx, "guide-write");
    let first = world.view.read_with(cx, |view, _| view.guide());
    let GuideRun::Writing { run, .. } = first.read_with(cx, |guide, _| guide.run().clone()) else {
        panic!("a run is writing");
    };
    wait_until(cx, "the run to start", |_| {
        world.work.path().join("out/started").exists()
    });

    let review = world.view.read_with(cx, |view, _| view.review());
    let original = review.read_with(cx, |review, _| review.host().comparison.clone());
    for comparison in [Comparison::Staged, original] {
        world
            .view
            .update_in(cx, |view, window, cx| view.select_comparison(comparison, window, cx));
        cx.run_until_parked();
    }

    let guide = world.view.read_with(cx, |view, _| view.guide());
    assert_ne!(guide.entity_id(), first.entity_id(), "the session was rebuilt");
    assert!(
        matches!(guide.read_with(cx, |guide, _| guide.run().clone()), GuideRun::Writing { run: adopted, .. } if adopted == run),
        "the new session shows the live run"
    );
    guide.update(cx, |guide, cx| guide.write(cx));
    assert!(
        matches!(guide.read_with(cx, |guide, _| guide.run().clone()), GuideRun::Writing { run: same, .. } if same == run),
        "a second write is refused while writing"
    );

    let files = diff_files(&world, cx);
    let written = demo_guide(world.repo.path(), &files);
    let review = world.view.read_with(cx, |view, _| view.review());
    let session = review.read_with(cx, |review, _| review.session_id());
    world
        .store
        .save_draft(session, run, ReviewModel::STANDARD, &written)
        .unwrap();
    world.store.complete_draft(run, &written).unwrap();
    std::fs::write(world.work.path().join("out/go"), "").unwrap();
    wait_until(cx, "the guide to load", |cx| {
        guide.read_with(cx, |guide, _| guide.stored().is_some())
    });
    assert_eq!(guide.read_with(cx, |guide, _| guide.run().clone()), GuideRun::Idle);
}

#[gpui::test]
fn the_reading_stays_until_the_new_one_lands_and_a_stale_result_is_dropped(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide(&world, cx);
    show_guide(&world, cx);
    let guide = world.view.read_with(cx, |view, _| view.guide());
    let chapters = |cx: &mut VisualTestContext| guide.read_with(cx, |guide, _| guide.reading().chapters.len());
    assert_eq!(chapters(cx), 2);

    let all = diff_files(&world, cx);
    let without: Vec<FileDiff> = all
        .iter()
        .filter(|file| file.path != "src/staged.rs")
        .cloned()
        .collect();
    let pane = world.view.read_with(cx, |view, _| view.pane());
    pane.update(cx, |pane, cx| pane.set_files(without.clone(), cx));
    assert_eq!(chapters(cx), 2, "the last good reading stays until the new one lands");
    cx.run_until_parked();
    assert_eq!(chapters(cx), 1);

    pane.update(cx, |pane, cx| pane.set_files(all.clone(), cx));
    pane.update(cx, |pane, cx| pane.set_files(without, cx));
    pane.update(cx, |pane, cx| pane.set_files(all, cx));
    cx.run_until_parked();
    assert_eq!(chapters(cx), 2, "only the last diff counts");
}

#[gpui::test]
fn the_banner_keeps_the_commit_count_while_the_diff_keeps_changing(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let behind = diffpilot_core::git::run(world.repo.path(), &["rev-parse", "HEAD~1"])
        .unwrap()
        .trim()
        .to_string();
    store_guide_with(&world, cx, |guide| guide.head = Some(behind));
    show_guide(&world, cx);
    let guide = world.view.read_with(cx, |view, _| view.guide());
    let text = |cx: &mut VisualTestContext| guide.read_with(cx, |guide, _| guide.out_of_date_text());
    let pane = world.view.read_with(cx, |view, _| view.pane());
    let edit = |cx: &mut VisualTestContext, suffix: &str| {
        let mut files = diff_files(&world, cx);
        let changed = files.iter_mut().find(|file| file.path == "base.txt").unwrap();
        changed.hunks[0].lines[0].text.push_str(suffix);
        pane.update(cx, |pane, cx| pane.set_files(files, cx));
    };

    edit(cx, " one");
    cx.run_until_parked();
    assert!(guide.read_with(cx, |guide, _| guide.out_of_date()));
    assert_eq!(text(cx), "The branch moved 1 commit after Claude wrote this guide.");

    edit(cx, " two");
    assert_eq!(text(cx), "The branch moved 1 commit after Claude wrote this guide.");
    cx.run_until_parked();
    assert_eq!(text(cx), "The branch moved 1 commit after Claude wrote this guide.");
}
