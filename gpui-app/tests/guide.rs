use diffpilot::diff_pane::rows::{BlockId, RowKind};
use diffpilot::guide::session::{GuideRun, GuideSession, Step};
use diffpilot::repo_view::{Nav, RepoView};
use diffpilot::runs::Runs;
use diffpilot::testing::{self, demo_guide, fixture, open_repo_in_workspace};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::FileDiff;
use diffpilot_core::guide::{Diagram, DiagramKind, Guide, Note, Span, StopKind};
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
    settings: Arc<Settings>,
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
printf '%s\n' "$@" > '{out}/args'
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
    let settings = Arc::new(Settings::in_memory());
    let (view, vcx) = open_repo_in_workspace(
        cx,
        repo.path(),
        settings.clone(),
        Arc::new(ViewedStore::in_memory()),
        store.clone(),
    );
    (
        World {
            view,
            store,
            settings,
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
fn the_overview_bar_has_no_link_button(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide(&world, cx);
    show_guide(&world, cx);
    testing::draw(cx);
    assert!(testing::shows(cx, "guide-rewrite"), "the overview bar is drawn");
    assert!(!testing::shows(cx, "guide-linked"), "linking lives in the title bar");
}

#[gpui::test]
fn a_folder_linked_by_path_is_passed_to_the_guide_run(app: &mut TestAppContext) {
    let (world, cx) = open(app, true);
    show_guide(&world, cx);
    let shared = tempfile::tempdir().unwrap();
    let folder = shared.path().canonicalize().unwrap();

    world
        .settings
        .set_linked_folders(&world.repo.path().canonicalize().unwrap(), vec![folder.clone()]);
    let name = folder.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        testing::shows(cx, &format!("guide-linked-chip:{name}")),
        "the start card lists the linked folder"
    );

    click(cx, "guide-write");
    let started = world.work.path().join("out/started");
    wait_until(cx, "the run to start", |_| started.exists());
    let args = std::fs::read_to_string(world.work.path().join("out/args")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    let at = args
        .iter()
        .position(|arg| *arg == "--add-dir")
        .expect("--add-dir is passed");
    assert_eq!(args[at + 1], folder.display().to_string());
    std::fs::write(world.work.path().join("out/go"), "").unwrap();
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
        cx.debug_bounds("guide-review-count").is_some(),
        "the inspector shows the review path in a chapter with stops"
    );
    assert!(cx.debug_bounds("guide-mark-continue").is_none());
}

#[gpui::test]
fn mark_viewed_and_continue_marks_a_chapter_without_stops_and_opens_the_next(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide(&world, cx);
    show_guide(&world, cx);
    let guide = world.view.read_with(cx, |view, _| view.guide());
    guide.update(cx, |guide, cx| guide.go(Step::Chapter(1), cx));
    cx.run_until_parked();
    click(cx, "guide-mark-continue");
    cx.run_until_parked();

    assert_eq!(
        guide.read_with(cx, |guide, _| guide.step()),
        Step::Overview,
        "past the last chapter is the overview"
    );
    let viewed = guide.read_with(cx, |guide, _| guide.viewed().clone());
    assert!(
        viewed.contains("base.txt") && viewed.contains("docs/committed.md"),
        "{viewed:?}"
    );
    assert_eq!(guide.read_with(cx, |guide, _| guide.read_count()), 1);
    assert_eq!(
        guide.read_with(cx, |guide, _| guide.first_unread()),
        0,
        "the chapter with stops is read when its stops are reviewed"
    );
}

#[gpui::test]
fn the_guide_follows_the_branch_to_another_comparison_and_shows_as_out_of_date(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide(&world, cx);
    show_guide(&world, cx);
    assert!(cx.debug_bounds("guide-out-of-date").is_none());

    world
        .view
        .update_in(cx, |view, window, cx| view.show_nav(Nav::Files, window, cx));
    cx.run_until_parked();
    testing::compare_to(cx, "Working Tree Changes Only");
    testing::draw(cx);
    show_guide(&world, cx);
    testing::draw(cx);

    let guide = world.view.read_with(cx, |view, _| view.guide());
    assert!(
        guide.read_with(cx, |guide, _| guide.stored().is_some()),
        "the guide written against the branch base is still the guide of this branch"
    );
    assert!(
        cx.debug_bounds("guide-out-of-date").is_some(),
        "it describes another diff, so it shows as out of date"
    );
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
    let guide = world.view.read_with(cx, |view, _| view.guide());
    guide.update(cx, |guide, cx| guide.go(Step::Chapter(1), cx));
    let step = |cx: &mut VisualTestContext| guide.read_with(cx, |guide, _| guide.step());

    testing::press(cx, "]");
    assert_eq!(
        step(cx),
        Step::Overview,
        "a chapter with no stops steps to the next chapter, past the last is the overview"
    );

    testing::press(cx, "[");
    assert_eq!(step(cx), Step::Overview, "the keys do nothing outside a chapter");

    guide.update(cx, |guide, cx| guide.go(Step::Chapter(1), cx));
    testing::press(cx, "[");
    assert_eq!(step(cx), Step::Chapter(0));
}

fn read_stops(guide: &mut Guide) {
    guide.chapters[0].notes[0].kind = StopKind::Read;
    guide.chapters[0].notes[1].check = Some("Is the value ever negative?".into());
}

fn open_first_chapter(world: &World, cx: &mut VisualTestContext) -> Entity<GuideSession> {
    store_guide_with(world, cx, read_stops);
    show_guide(world, cx);
    click(cx, "guide-start-reading");
    world.view.read_with(cx, |view, _| view.guide())
}

fn stop(guide: &Entity<GuideSession>, cx: &mut VisualTestContext) -> Option<usize> {
    guide.read_with(cx, |guide, _| guide.current_stop())
}

fn card_line(world: &World, cx: &mut VisualTestContext, slot: u32) -> Option<u32> {
    let pane = world.view.read_with(cx, |view, _| view.pane());
    pane.read_with(cx, |pane, _| {
        let state = pane.state();
        let row = state.rows.block_row(slot)?;
        let RowKind::Line { hunk, line } = state.rows.rows[row - 1].kind else {
            return None;
        };
        state.files[0].hunks[hunk as usize].lines[line as usize].new
    })
}

#[gpui::test]
fn entering_a_chapter_shows_the_first_stop_card_and_the_key_moves_it_to_the_next_stop(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let guide = open_first_chapter(&world, cx);
    assert_eq!(stop(&guide, cx), Some(0));
    assert!(testing::shows(cx, "pane:stop-card:1"), "the first stop is expanded");
    assert!(testing::shows(cx, "pane:stop-kind"));
    assert!(testing::shows(cx, "pane:stop-looks-good"));
    assert_eq!(card_line(&world, cx, 0), Some(5));

    testing::press(cx, "]");
    assert_eq!(stop(&guide, cx), Some(1));
    assert!(testing::shows(cx, "pane:stop-card:2"));
    assert!(testing::shows(cx, "pane:stop-check:Is the value ever negative?"));
    assert!(testing::shows(cx, "pane:stop-critical"), "the second note is critical");
    assert_eq!(card_line(&world, cx, 1), Some(7));

    testing::press(cx, "[");
    assert_eq!(stop(&guide, cx), Some(0));
}

#[gpui::test]
fn v_marks_the_stop_reviewed_and_moves_on_and_the_mark_survives_reopening(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let guide = open_first_chapter(&world, cx);

    testing::press(cx, "v");
    assert_eq!(stop(&guide, cx), Some(1));
    assert!(guide.read_with(cx, |guide, _| guide.reviewed().contains(&(0, 0))));
    let id = guide.read_with(cx, |guide, _| guide.stored().unwrap().id);
    assert!(world.store.reviewed_stops(id).unwrap().contains(&(0, 0)));
    assert_eq!(guide.read_with(cx, |guide, _| guide.read_count()), 0);

    testing::press(cx, "v");
    assert_eq!(stop(&guide, cx), Some(1), "at the end the cursor stays");
    assert_eq!(
        guide.read_with(cx, |guide, _| guide.read_count()),
        1,
        "all stops reviewed reads the chapter"
    );

    let review = world.view.read_with(cx, |view, _| view.review());
    let original = review.read_with(cx, |review, _| review.host().comparison.clone());
    for comparison in [Comparison::Staged, original] {
        world
            .view
            .update_in(cx, |view, window, cx| view.select_comparison(comparison, window, cx));
        cx.run_until_parked();
    }
    let reopened = world.view.read_with(cx, |view, _| view.guide());
    assert_ne!(reopened.entity_id(), guide.entity_id(), "the session was rebuilt");
    assert_eq!(reopened.read_with(cx, |guide, _| guide.reviewed().len()), 2);
    reopened.update(cx, |guide, cx| guide.go(Step::Chapter(0), cx));
    assert_eq!(
        stop(&reopened, cx),
        Some(0),
        "every stop is reviewed, so the entry is the first stop"
    );
}

#[gpui::test]
fn a_skipped_stop_is_the_next_open_one_after_looks_good(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let guide = open_first_chapter(&world, cx);

    testing::click(cx, "pane:stop-skip");
    assert_eq!(stop(&guide, cx), Some(1));
    assert!(
        guide.read_with(cx, |guide, _| guide.reviewed().is_empty()),
        "skip marks nothing"
    );
    testing::press(cx, "v");
    assert_eq!(stop(&guide, cx), Some(0), "the skipped stop is still open");
}

#[gpui::test]
fn entering_a_chapter_starts_at_the_first_unreviewed_stop(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let guide = open_first_chapter(&world, cx);
    testing::press(cx, "v");
    guide.update(cx, |guide, cx| guide.go(Step::Overview, cx));
    guide.update(cx, |guide, cx| guide.go(Step::Chapter(0), cx));
    assert_eq!(stop(&guide, cx), Some(1));
}

#[gpui::test]
fn the_last_stop_hands_the_key_to_the_next_chapter_and_the_first_stop_to_the_previous_one(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let guide = open_first_chapter(&world, cx);
    testing::press(cx, "]");
    assert_eq!(stop(&guide, cx), Some(1));

    testing::press(cx, "]");
    assert_eq!(guide.read_with(cx, |guide, _| guide.step()), Step::Chapter(1));
    assert_eq!(pane_paths(&world, cx), ["base.txt", "docs/committed.md"]);

    testing::press(cx, "[");
    assert_eq!(guide.read_with(cx, |guide, _| guide.step()), Step::Chapter(0));
    assert_eq!(stop(&guide, cx), Some(0));
    testing::press(cx, "[");
    assert_eq!(
        guide.read_with(cx, |guide, _| guide.step()),
        Step::Overview,
        "before the first stop of the first chapter"
    );
}

#[gpui::test]
fn the_looks_good_button_works_like_the_key(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let guide = open_first_chapter(&world, cx);

    testing::click(cx, "pane:stop-looks-good");
    assert_eq!(stop(&guide, cx), Some(1));
    assert!(guide.read_with(cx, |guide, _| guide.reviewed().contains(&(0, 0))));
    assert!(testing::shows(cx, "pane:stop-card:2"));
}

#[gpui::test]
fn the_comment_button_starts_a_comment_on_the_stop_line(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    open_first_chapter(&world, cx);
    testing::press(cx, "]");
    testing::click(cx, "pane:stop-comment");
    assert!(
        testing::shows(cx, "thread:cancel:src/staged.rs:7"),
        "a draft opens on the second stop's line"
    );
}

#[gpui::test]
fn a_note_in_a_linked_folder_is_a_stop_and_the_pane_shows_no_card_for_it(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide_with(&world, cx, |guide| {
        let mut linked = guide.chapters[0].notes[0].clone();
        linked.path = "lib/util.rs".into();
        linked.folder = Some("/tmp/shared".into());
        guide.chapters[0].notes.insert(1, linked);
    });
    show_guide(&world, cx);
    click(cx, "guide-start-reading");
    let guide = world.view.read_with(cx, |view, _| view.guide());
    testing::press(cx, "]");
    assert_eq!(stop(&guide, cx), Some(1), "the linked note is the second stop");
    assert_eq!(pane_paths(&world, cx), ["src/staged.rs"], "no wrong file is revealed");
    let pane = world.view.read_with(cx, |view, _| view.pane());
    assert_eq!(
        pane.read_with(cx, |pane, _| pane.state().blocks.len()),
        2,
        "the two diff notes only"
    );
    testing::press(cx, "]");
    assert_eq!(stop(&guide, cx), Some(2));
    assert!(testing::shows(cx, "pane:stop-card:3"));
}

fn flowchart() -> Diagram {
    Diagram {
        title: Some("Undo path".into()),
        kind: DiagramKind::Flowchart,
        source: "flowchart LR\n  A[Browser]:::changed -->|POST| B{Valid}\n  B --> C(Handler):::added".into(),
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
fn expand_opens_the_diagram_at_full_size_and_escape_or_close_shuts_it(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide_with(&world, cx, |guide| guide.diagrams = vec![flowchart()]);
    show_guide(&world, cx);
    assert!(cx.debug_bounds("guide-diagram-sheet").is_none());

    let card = cx.debug_bounds("guide-diagram-canvas").expect("the diagram is drawn");
    click(cx, "guide-diagram-expand");
    let sheet = cx
        .debug_bounds("guide-diagram-sheet:canvas")
        .expect("the sheet draws the diagram");
    assert!(sheet.size.width >= card.size.width, "{sheet:?} vs {card:?}");
    assert!(sheet.size.height >= card.size.height, "{sheet:?} vs {card:?}");

    testing::press(cx, "escape");
    assert!(testing::shows(cx, "guide-diagram-sheet:closing"));
    testing::wait_for_motion(cx);

    click(cx, "guide-diagram-expand");
    click(cx, "guide-diagram-sheet:close");
    testing::wait_for_motion(cx);
    assert!(!testing::shows(cx, "guide-diagram-source"));
    click(cx, "guide-diagram-view:Source");
    assert!(
        testing::shows(cx, "guide-diagram-source"),
        "the closed sheet no longer covers the card"
    );
}

#[gpui::test]
fn a_diagram_that_cannot_be_drawn_falls_back_to_its_source_with_a_note(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    store_guide_with(&world, cx, |guide| {
        guide.diagrams = vec![Diagram {
            title: None,
            kind: DiagramKind::Other("gantt".into()),
            source: "gantt\n  title Plan".into(),
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

const LONG: &str = "This sentence is long on purpose so that it has to wrap onto several lines inside a narrow column, \
and it carries `popover()` and `paint_path` code spans";

fn long_guide(guide: &mut Guide) {
    guide.overview.before = Some(LONG.into());
    guide.overview.after = Some(LONG.replace("long", "longer"));
    guide.chapters[0].summary = LONG.into();
    guide.chapters[0].points = vec![LONG.into(), LONG.into()];
    guide.chapters[0].focus = vec![LONG.into()];
    guide.chapters[1].title = "Changelog, xtask features, screenshots and a lot more words".into();
}

fn inside(inner: gpui::Bounds<gpui::Pixels>, outer: gpui::Bounds<gpui::Pixels>) -> bool {
    let slack = gpui::px(0.5);
    inner.left() >= outer.left() - slack
        && inner.right() <= outer.right() + slack
        && inner.top() >= outer.top() - slack
        && inner.bottom() <= outer.bottom() + slack
}

#[gpui::test]
fn long_overview_text_wraps_inside_its_cards_and_chapter_rows(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(900.)));
    store_guide_with(&world, cx, long_guide);
    show_guide(&world, cx);

    let article = testing::bounds(cx, "guide-article");
    let before = testing::bounds(cx, "guide-before");
    let after = testing::bounds(cx, "guide-after");
    assert!(
        inside(before, article) && inside(after, article),
        "{before:?} {after:?} {article:?}"
    );
    assert!(
        before.right() <= after.left(),
        "the cards do not overlap: {before:?} {after:?}"
    );
    assert!(
        (before.size.width - after.size.width).abs() < gpui::px(1.),
        "equal columns: {before:?} {after:?}"
    );
    for (card, text) in [(before, "guide-before-text"), (after, "guide-after-text")] {
        let text = testing::bounds(cx, text);
        assert!(inside(text, card), "{text:?} is inside {card:?}");
        assert!(text.size.height > gpui::px(40.), "the text wraps: {text:?}");
    }

    let summary = testing::bounds(cx, "guide-chapter-summary:0");
    let stat = testing::bounds(cx, "guide-chapter-stat:0");
    assert!(inside(summary, article), "{summary:?} {article:?}");
    assert!(
        summary.right() <= stat.left(),
        "the summary stops before the stat: {summary:?} {stat:?}"
    );
    assert!(summary.size.height > gpui::px(40.), "the summary wraps: {summary:?}");
    assert!(inside(stat, article), "{stat:?} {article:?}");
}

#[gpui::test]
fn long_inspector_text_wraps_and_a_long_next_label_stays_in_its_button(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(1800.)));
    store_guide_with(&world, cx, long_guide);
    show_guide(&world, cx);
    let guide = world.view.read_with(cx, |view, _| view.guide());
    guide.update(cx, |guide, cx| guide.go(Step::Chapter(0), cx));
    cx.run_until_parked();

    let inspector = testing::bounds(cx, "guide-inspector");
    for name in ["guide-point:0", "guide-point:1", "guide-focus:0"] {
        let bullet = testing::bounds(cx, name);
        assert!(inside(bullet, inspector), "{name} {bullet:?} is inside {inspector:?}");
        assert!(bullet.size.height > gpui::px(40.), "{name} wraps: {bullet:?}");
    }
    let button = testing::bounds(cx, "guide-next-chapter");
    let label = testing::bounds(cx, "guide-next-label");
    assert!(inside(button, inspector), "{button:?} {inspector:?}");
    assert!(inside(label, button), "{label:?} is inside {button:?}");
}

fn map_chart() -> Diagram {
    Diagram {
        title: Some("Pipeline".into()),
        kind: DiagramKind::Flowchart,
        source: "flowchart LR\n  A[Parser] --> B[Writer]\n  B --> C[Output]".into(),
    }
}

fn path_guide(guide: &mut Guide) {
    read_stops(guide);
    guide.chapters[0].diagrams = vec![map_chart()];
    guide.chapters[0].notes[0].node = Some("A".into());
    guide.chapters[0].notes[1].node = Some("B".into());
    let moved = guide.chapters[1].files.pop().unwrap();
    guide.chapters[0].files.push(moved);
}

fn open_path_chapter(world: &World, cx: &mut VisualTestContext) -> Entity<GuideSession> {
    cx.simulate_resize(gpui::size(gpui::px(1300.), gpui::px(1100.)));
    store_guide_with(world, cx, path_guide);
    show_guide(world, cx);
    click(cx, "guide-start-reading");
    world.view.read_with(cx, |view, _| view.guide())
}

#[gpui::test]
fn the_panel_lists_the_stops_and_a_row_click_moves_the_stop_card(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let guide = open_path_chapter(&world, cx);
    assert!(testing::shows(cx, "guide-review-count"));
    assert!(testing::shows(cx, "guide-stop-row:1"));
    assert!(testing::shows(cx, "guide-stop-row:2"));
    assert!(testing::shows(cx, "pane:stop-card:1"));
    assert!(
        !testing::shows(cx, "guide-stop-check:Is the value ever negative?"),
        "only the current row shows its check"
    );

    testing::click(cx, "guide-stop-row:2");
    assert_eq!(stop(&guide, cx), Some(1));
    assert!(testing::shows(cx, "pane:stop-card:2"));
    assert!(testing::shows(cx, "guide-stop-check:Is the value ever negative?"));
    assert_eq!(card_line(&world, cx, 1), Some(7));

    testing::click(cx, "guide-stop-prev");
    assert_eq!(stop(&guide, cx), Some(0));
    testing::click(cx, "guide-stop-next");
    assert_eq!(stop(&guide, cx), Some(1));
}

#[gpui::test]
fn only_a_critical_stop_has_a_danger_mark_in_the_panel_list(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    open_path_chapter(&world, cx);
    assert!(testing::shows(cx, "guide-stop-row:2"));
    assert!(testing::shows(cx, "guide-stop-critical:2"));
    assert!(testing::shows(cx, "guide-stop-row:1"));
    assert!(!testing::shows(cx, "guide-stop-critical:1"));
}

fn fill_width(cx: &mut VisualTestContext, selector: &str) -> f32 {
    f32::from(testing::settled_bounds(cx, selector).size.width)
}

#[gpui::test]
fn reviewing_a_stop_checks_its_row_and_fills_the_navigator_bar(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let guide = open_path_chapter(&world, cx);
    assert!(fill_width(cx, "guide-chapter-bar:1") < 0.5);
    assert!(testing::shows(cx, "guide-chapter-progress:1"));

    testing::press(cx, "v");
    assert!(testing::shows(cx, "guide-stop-done:1"));
    let half = fill_width(cx, "guide-chapter-bar:1");
    assert!(
        (half - 14.0).abs() < 1.0,
        "one of two stops fills half of the bar: {half}"
    );
    assert!(!testing::shows(cx, "guide-chapter-reviewed"));

    testing::press(cx, "v");
    assert!(testing::shows(cx, "guide-stop-done:2"));
    assert!((fill_width(cx, "guide-chapter-bar:1") - 28.0).abs() < 1.0);
    assert!(
        testing::shows(cx, "guide-chapter-reviewed"),
        "every stop is reviewed, so the chapter is"
    );

    testing::click(cx, "guide-chapter-reviewed-next");
    assert_eq!(guide.read_with(cx, |guide, _| guide.step()), Step::Chapter(1));
}

#[gpui::test]
fn a_map_node_with_stops_moves_to_its_first_stop(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let guide = open_path_chapter(&world, cx);
    assert!(testing::shows(cx, "guide-map"));
    assert_eq!(stop(&guide, cx), Some(0));

    testing::click(cx, "guide-map-node:B");
    assert_eq!(stop(&guide, cx), Some(1));
    assert!(testing::shows(cx, "pane:stop-card:2"));

    testing::click(cx, "guide-map-node:A");
    assert_eq!(stop(&guide, cx), Some(0));
    assert!(
        !testing::shows(cx, "guide-map-node:C"),
        "a node without stops is not a button"
    );
}

#[gpui::test]
fn the_overview_counts_the_stops_and_start_review_opens_the_first_open_one(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    cx.simulate_resize(gpui::size(gpui::px(1300.), gpui::px(1100.)));
    store_guide_with(&world, cx, |guide| {
        path_guide(guide);
        let mut extra = guide.chapters[0].notes[0].clone();
        extra.path = "base.txt".into();
        extra.span = None;
        extra.node = None;
        guide.chapters[1].notes.push(extra);
    });
    show_guide(&world, cx);
    let guide = world.view.read_with(cx, |view, _| view.guide());
    assert!(testing::shows(cx, "guide-review-banner"));
    assert!(testing::shows(cx, "guide-review-counts"));
    assert!(testing::shows(cx, "guide-chapter-stops:0"));
    assert!(testing::shows(cx, "guide-chapter-stops:1"));

    testing::click(cx, "guide-review-start");
    assert_eq!(guide.read_with(cx, |guide, _| guide.step()), Step::Chapter(0));
    assert_eq!(stop(&guide, cx), Some(0));

    testing::press(cx, "v");
    testing::press(cx, "v");
    guide.update(cx, |guide, cx| guide.go(Step::Overview, cx));
    testing::click(cx, "guide-review-start");
    assert_eq!(
        guide.read_with(cx, |guide, _| guide.step()),
        Step::Chapter(1),
        "chapter one is reviewed, so the review starts at the next chapter with an open stop"
    );

    guide.update(cx, |guide, cx| guide.go(Step::Overview, cx));
    testing::click(cx, "guide-chapter-review:0");
    assert_eq!(guide.read_with(cx, |guide, _| guide.step()), Step::Chapter(0));
}

#[gpui::test]
fn a_chapter_file_that_no_stop_points_at_is_listed_as_not_on_the_path(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    open_path_chapter(&world, cx);
    assert!(testing::shows(cx, "guide-not-on-path"));
    assert!(testing::shows(cx, "guide-off-path:docs/committed.md"));
    assert!(
        !testing::shows(cx, "guide-off-path:src/staged.rs"),
        "the stops point at this file"
    );
}

fn top_file(world: &World, cx: &mut VisualTestContext) -> (f32, Option<String>) {
    let pane = world.view.read_with(cx, |view, _| view.pane());
    pane.read_with(cx, |pane, _| {
        let state = pane.state();
        let row = state.tops.row_at(state.scroll());
        let file = row.map(|row| state.files[state.rows.rows[row].file as usize].path.clone());
        (state.scroll(), file)
    })
}

fn collapsed(world: &World, cx: &mut VisualTestContext, path: &str) -> bool {
    let pane = world.view.read_with(cx, |view, _| view.pane());
    pane.read_with(cx, |pane, _| pane.state().collapsed.contains(path))
}

#[gpui::test]
fn a_click_on_a_file_off_the_path_shows_that_file_on_its_own(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let guide = open_path_chapter(&world, cx);
    assert_eq!(stop(&guide, cx), Some(0));

    testing::click(cx, "guide-off-path:docs/committed.md");
    assert!(testing::shows(cx, "guide-off-path-open:docs/committed.md"));
    assert_eq!(stop(&guide, cx), None, "no stop row stays current");
    assert_eq!(pane_paths(&world, cx), ["docs/committed.md"]);
    assert_eq!(top_file(&world, cx), (0.0, Some("docs/committed.md".into())));

    testing::click(cx, "pane:viewed:docs/committed.md");
    assert!(collapsed(&world, cx, "docs/committed.md"));
    testing::click(cx, "guide-off-path-open:docs/committed.md");
    assert!(
        !collapsed(&world, cx, "docs/committed.md"),
        "a second click opens the file again"
    );
    assert_eq!(top_file(&world, cx), (0.0, Some("docs/committed.md".into())));

    testing::click(cx, "guide-file-row:src/staged.rs");
    assert_eq!(pane_paths(&world, cx), ["src/staged.rs", "docs/committed.md"]);
    assert_eq!(top_file(&world, cx).1.as_deref(), Some("src/staged.rs"));

    testing::click(cx, "guide-off-path:docs/committed.md");
    testing::click(cx, "guide-stop-row:2");
    assert_eq!(stop(&guide, cx), Some(1));
    assert_eq!(pane_paths(&world, cx), ["src/staged.rs", "docs/committed.md"]);
    assert!(testing::shows(cx, "pane:stop-card:2"));
}

const READER: &str = "use std::fmt;\n\nstruct Reader {\n    path: String,\n}\n\nimpl Reader {\n    fn read(&self) -> String {\n        std::fs::read_to_string(&self.path).unwrap()\n    }\n}\n";

fn linked_folder() -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    testing::write(folder.path(), "src/reader.rs", READER);
    folder
}

fn linked_at(at: usize, folder: &Path, file: &str, anchor: &str) -> impl FnOnce(&mut Guide) {
    let (folder, file, anchor) = (folder.to_path_buf(), file.to_string(), anchor.to_string());
    move |guide| {
        read_stops(guide);
        guide.context.push(folder.clone());
        guide.chapters[0].notes.insert(
            at,
            Note {
                path: file,
                span: Some(Span {
                    side: diffpilot_core::threads::Side::New,
                    line: 9,
                    end_line: None,
                    anchor_text: Some(anchor),
                }),
                text: "Readers call this once per file. They never cache it.".into(),
                critical: false,
                kind: StopKind::Check,
                check: Some("No caller keeps its own copy.".into()),
                node: None,
                folder: Some(folder),
            },
        );
    }
}

fn open_with_linked(
    world: &World,
    cx: &mut VisualTestContext,
    at: usize,
    file: &str,
    anchor: &str,
    folder: &Path,
) -> Entity<GuideSession> {
    store_guide_with(world, cx, linked_at(at, folder, file, anchor));
    show_guide(world, cx);
    click(cx, "guide-start-reading");
    world.view.read_with(cx, |view, _| view.guide())
}

#[gpui::test]
fn a_linked_stop_shows_the_file_with_its_lines_banded_and_v_returns_to_the_diff(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let folder = linked_folder();
    let guide = open_with_linked(
        &world,
        cx,
        1,
        "src/reader.rs",
        "        std::fs::read_to_string(&self.path).unwrap()",
        folder.path(),
    );

    testing::press(cx, "]");
    assert_eq!(stop(&guide, cx), Some(1));
    assert!(testing::shows(cx, "pane:linked-pill"));
    assert!(testing::shows(cx, "pane:linked-context"));
    assert!(
        testing::shows(
            cx,
            "pane:linked-band:9:        std::fs::read_to_string(&self.path).unwrap()"
        ),
        "the stop's line is banded"
    );
    assert!(
        testing::shows(cx, "pane:linked-line:5"),
        "four lines of context sit above"
    );
    assert!(
        testing::shows(cx, "pane:linked-line:11"),
        "and below, up to the end of the file"
    );
    assert!(
        !testing::shows(cx, "pane:linked-line:4"),
        "nothing further above (never drawn in this window)"
    );
    assert!(testing::shows(cx, "pane:stop-card:2"));
    assert!(testing::shows(cx, "pane:stop-looks-good"));
    assert!(!testing::shows(cx, "pane:linked-moved"));

    testing::press(cx, "v");
    assert_eq!(stop(&guide, cx), Some(2));
    assert!(guide.read_with(cx, |guide, _| guide.reviewed().contains(&(0, 1))));
    assert!(
        testing::shows(cx, "pane:stop-card:3"),
        "the diff pane shows the next card"
    );
}

#[gpui::test]
fn a_linked_stop_has_no_comment_button(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let folder = linked_folder();
    let guide = open_with_linked(
        &world,
        cx,
        0,
        "src/reader.rs",
        "        std::fs::read_to_string(&self.path).unwrap()",
        folder.path(),
    );
    assert_eq!(stop(&guide, cx), Some(0));
    assert!(testing::shows(cx, "pane:stop-card:1"));
    assert!(testing::shows(cx, "pane:stop-skip"));
    assert!(
        !testing::shows(cx, "pane:stop-comment"),
        "a file outside the diff has no line to comment on"
    );
}

#[gpui::test]
fn the_looks_good_button_on_a_linked_stop_marks_it_and_moves_on(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let folder = linked_folder();
    let guide = open_with_linked(
        &world,
        cx,
        0,
        "src/reader.rs",
        "        std::fs::read_to_string(&self.path).unwrap()",
        folder.path(),
    );
    testing::press(cx, "]");
    testing::click(cx, "pane:stop-looks-good");
    assert_eq!(stop(&guide, cx), Some(2));
    assert!(testing::shows(cx, "pane:stop-card:3"));
    testing::press(cx, "[");
    assert_eq!(stop(&guide, cx), Some(1));
    assert!(guide.read_with(cx, |guide, _| guide.reviewed().contains(&(0, 1))));
}

#[gpui::test]
fn a_linked_stop_whose_file_is_gone_says_so_and_keeps_the_card(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let folder = linked_folder();
    let guide = open_with_linked(&world, cx, 1, "src/missing.rs", "x", folder.path());
    testing::press(cx, "]");
    assert_eq!(stop(&guide, cx), Some(1));
    assert!(testing::shows(cx, "pane:linked-gone"));
    assert!(testing::shows(cx, "pane:linked-pill"));
    assert!(!testing::shows(cx, "pane:linked-band:9:x"));
    assert!(testing::shows(cx, "pane:stop-card:2"));
}

#[gpui::test]
fn a_linked_stop_whose_lines_changed_shows_the_file_with_a_moved_note(app: &mut TestAppContext) {
    let (world, cx) = open(app, false);
    let folder = linked_folder();
    let guide = open_with_linked(&world, cx, 1, "src/reader.rs", "let text = old_code();", folder.path());
    testing::press(cx, "]");
    assert_eq!(stop(&guide, cx), Some(1));
    assert!(testing::shows(cx, "pane:linked-moved"));
    assert!(testing::shows(cx, "pane:linked-line:5"));
    assert!(testing::shows(cx, "pane:stop-card:2"));
}
