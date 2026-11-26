use diffpilot::diff_pane::DiffPane;
use diffpilot::diff_pane::layout::{LINE, SPLIT_DIVIDER, THREAD, ThreadFrame, column_width};
use diffpilot::diff_pane::rows::RowKind;
use diffpilot::review::{ReviewHost, ReviewSession, metrics};
use diffpilot_core::cancel::CancelToken;
use diffpilot_core::changes::FileStatus;
use diffpilot_core::claude::{Replier, ReviewError, Reviewer};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::{DiffLine, DiffMode, FileDiff, Hunk, LineKind};
use diffpilot_core::review::{Finding, ReviewRequest, ReviewResult, Severity};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::Store;
use diffpilot_core::threads::{ReviewThread, Side, ThreadAnchor, ThreadId};
use gpui::{AppContext as _, Entity, Modifiers, TestAppContext, VisualTestContext, point, px, size};
use gpui_component::Root;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct Stub(ReviewResult);

impl Reviewer for Stub {
    fn review(&self, _: &ReviewRequest, _: &CancelToken) -> Result<ReviewResult, ReviewError> {
        Ok(self.0.clone())
    }
}

#[derive(Default)]
struct Chunks(Mutex<Vec<async_channel::Sender<String>>>);

impl Chunks {
    fn send(&self, chunk: &str) {
        for sender in self.0.lock().unwrap().iter() {
            sender.send_blocking(chunk.into()).unwrap();
        }
    }
}

impl Replier for Chunks {
    fn reply(&self, _: &ReviewThread, _: &CancelToken) -> async_channel::Receiver<String> {
        let (sender, receiver) = async_channel::unbounded();
        self.0.lock().unwrap().push(sender);
        receiver
    }
}

fn file(path: &str, lines: usize) -> FileDiff {
    let mut diff = FileDiff::new(path, FileStatus::Modified);
    let lines: Vec<DiffLine> = (1..=lines as u32)
        .map(|number| DiffLine {
            kind: LineKind::Added,
            old: None,
            new: Some(number),
            text: format!("const value{number} = {number};"),
        })
        .collect();
    diff.additions = lines.len() as u32;
    diff.hunks.push(Hunk {
        header: "@@ -0,0 +1 @@".into(),
        lines,
    });
    diff
}

fn finding(path: &str, line: Option<u32>, severity: Severity, title: &str) -> Finding {
    Finding {
        path: path.into(),
        line,
        severity,
        title: title.into(),
        body: format!("{title} body"),
    }
}

fn review() -> ReviewResult {
    ReviewResult {
        summary: "Looks fine.".into(),
        findings: vec![
            finding("a.ts", Some(2), Severity::MustFix, "Bad"),
            finding("b.ts", None, Severity::Nit, "Name"),
            finding("gone.py", Some(3), Severity::Question, "Used?"),
        ],
    }
}

struct Opened {
    pane: Entity<DiffPane>,
    session: Entity<ReviewSession>,
    replier: Arc<Chunks>,
}

fn open(cx: &mut TestAppContext) -> (Opened, &mut VisualTestContext) {
    cx.update(|cx| {
        diffpilot_ui_kit::init(cx);
        diffpilot::review::init(cx);
    });
    let replier = Arc::new(Chunks::default());
    let mut host = ReviewHost::new(Some(PathBuf::from("/repo")), Comparison::checked_out("main", true));
    host.branch_name = Some("feature".into());
    let session = {
        let replier = replier.clone();
        let store = Arc::new(Store::open_in_memory().unwrap());
        cx.new(move |_| {
            ReviewSession::new(
                host,
                Arc::new(Settings::in_memory()),
                store,
                Arc::new(Stub(review())),
                replier,
            )
        })
    };
    let mut pane = None;
    let handle = cx.add_window({
        let session = session.clone();
        |window, cx| {
            let view = cx.new(|cx| DiffPane::new(window, cx));
            view.update(cx, |view, cx| view.set_review(session, window, cx));
            pane = Some(view.clone());
            Root::new(view, window, cx)
        }
    });
    let cx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    let pane = pane.expect("the window builds the pane");
    pane.update(cx, |pane, cx| {
        pane.set_files(["a.ts", "b.ts", "c.ts"].map(|path| file(path, 40)).to_vec(), cx)
    });
    cx.run_until_parked();
    (Opened { pane, session, replier }, cx)
}

fn draw(pane: &Entity<DiffPane>, cx: &mut VisualTestContext) {
    let pane = pane.clone();
    cx.draw(point(px(0.0), px(0.0)), size(px(900.0), px(600.0)), move |_, _| pane);
    cx.run_until_parked();
}

fn row_height(opened: &Opened, cx: &mut VisualTestContext, id: ThreadId) -> f32 {
    opened
        .pane
        .read_with(cx, |pane, _| pane.state().thread_height(id).expect("a thread row"))
}

fn card_height(opened: &Opened, cx: &mut VisualTestContext, id: ThreadId, anchor: &ThreadAnchor) -> f32 {
    let card = opened
        .pane
        .read_with(cx, |pane, _| pane.card(id).cloned().expect("a card"));
    let (pane_width, split) = opened.pane.read_with(cx, |pane, _| {
        (pane.state().geometry().width, pane.state().split_width())
    });
    let width = px(ThreadFrame::of(anchor, split).card_width(pane_width));
    f32::from(cx.update(|window, cx| card.read(cx).height(width, window, cx)))
}

fn frame_of(opened: &Opened, cx: &mut VisualTestContext, anchor: &ThreadAnchor) -> ThreadFrame {
    let split = opened.pane.read_with(cx, |pane, _| pane.state().split_width());
    ThreadFrame::of(anchor, split)
}

fn run_review(opened: &Opened, cx: &mut VisualTestContext) -> Vec<ReviewThread> {
    opened.session.update(cx, |session, cx| session.start_review(cx));
    cx.run_until_parked();
    draw(&opened.pane, cx);
    opened
        .session
        .read_with(cx, |session, _| session.threads().cloned().collect())
}

#[gpui::test]
fn a_review_run_puts_its_findings_in_the_pane_with_the_height_of_their_cards(cx: &mut TestAppContext) {
    review_run_heights(cx, DiffMode::Unified);
}

#[gpui::test]
fn split_threads_are_measured_at_the_width_of_their_half(cx: &mut TestAppContext) {
    review_run_heights(cx, DiffMode::Split);
}

fn review_run_heights(cx: &mut TestAppContext, mode: DiffMode) {
    let (opened, cx) = open(cx);
    opened.pane.update(cx, |pane, cx| pane.set_mode(mode, cx));
    draw(&opened.pane, cx);
    let before = opened.pane.read_with(cx, |pane, _| pane.state().tops.total());
    let threads = run_review(&opened, cx);
    assert_eq!(threads.len(), 4, "the summary and three findings");
    let slots = opened.pane.read_with(cx, |pane, _| pane.state().threads.len());
    assert_eq!(slots, 4);

    let mut grown = 0.0;
    for thread in &threads {
        let measured = row_height(&opened, cx, thread.id);
        let frame = frame_of(&opened, cx, &thread.anchor);
        assert_eq!(
            measured,
            frame.row_height(card_height(&opened, cx, thread.id, &thread.anchor))
        );
        assert_ne!(measured, THREAD, "{:?} was measured", thread.anchor);
        grown += measured;
    }
    let total = opened.pane.read_with(cx, |pane, _| pane.state().tops.total());
    assert_eq!(total, before + grown);

    let nit = threads
        .iter()
        .find(|thread| {
            thread
                .finding
                .as_ref()
                .is_some_and(|meta| meta.severity == Severity::Nit)
        })
        .unwrap();
    let collapsed = 2.0 * metrics::BORDER + metrics::FINDING_HEADER;
    assert_eq!(
        row_height(&opened, cx, nit.id),
        frame_of(&opened, cx, &nit.anchor).row_height(collapsed)
    );

    let change = threads.iter().filter(|t| t.anchor == ThreadAnchor::Change).count();
    assert_eq!(change, 2, "the summary and the finding on a file outside the diff");
    let first = opened.pane.read_with(cx, |pane, _| pane.state().rows.rows[0].kind);
    assert!(matches!(first, diffpilot::diff_pane::rows::RowKind::Thread { .. }));

    let line = threads
        .iter()
        .find(|thread| matches!(thread.anchor, ThreadAnchor::Line { .. }))
        .unwrap();
    opened.pane.read_with(cx, |pane, _| {
        let state = pane.state();
        let row = state
            .thread_top(line.id)
            .map(|top| state.tops.row_at(top).unwrap())
            .unwrap();
        let shown = match state.rows.rows[row - 1].kind {
            RowKind::Line { line, .. } => (DiffMode::Unified, Some(line), None),
            RowKind::Pair { left, right, .. } => (DiffMode::Split, left, right),
            other => panic!("{other:?} is above the thread"),
        };
        assert_eq!(shown.0, mode);
        assert!(
            shown.1 == Some(1) || shown.2 == Some(1),
            "the line thread is under line 2: {shown:?}"
        );
    });
}

fn edited(path: &str) -> FileDiff {
    let line = |kind, old, new, text: &str| DiffLine {
        kind,
        old,
        new,
        text: text.into(),
    };
    let mut diff = FileDiff::new(path, FileStatus::Modified);
    diff.additions = 2;
    diff.deletions = 1;
    diff.hunks.push(Hunk {
        header: "@@ -1,3 +1,4 @@".into(),
        lines: vec![
            line(LineKind::Context, Some(1), Some(1), "keep"),
            line(LineKind::Deleted, Some(2), None, "old text"),
            line(LineKind::Added, None, Some(2), "new text"),
            line(LineKind::Added, None, Some(3), "more text"),
            line(LineKind::Context, Some(3), Some(4), "tail"),
        ],
    });
    diff
}

fn comment_on(opened: &Opened, cx: &mut VisualTestContext, hover: (f32, f32), plus: (f32, f32)) -> Vec<ThreadAnchor> {
    cx.simulate_mouse_move(point(px(hover.0), px(hover.1)), None, Modifiers::default());
    cx.run_until_parked();
    cx.simulate_click(point(px(plus.0), px(plus.1)), Modifiers::default());
    cx.run_until_parked();
    opened.session.read_with(cx, |session, _| {
        session.threads().map(|thread| thread.anchor.clone()).collect()
    })
}

#[gpui::test]
fn the_plus_works_in_both_halves_of_a_split_row_and_not_on_an_empty_cell(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    opened.pane.update(cx, |pane, cx| {
        pane.set_files(vec![edited("a.ts")], cx);
        pane.set_mode(DiffMode::Split, cx);
    });
    cx.simulate_resize(size(px(1000.0), px(700.0)));
    cx.run_until_parked();
    let width = opened.pane.read_with(cx, |pane, _| pane.state().geometry().width);
    assert_eq!(width, 1000.0);
    let right = column_width(width) + SPLIT_DIVIDER;
    let rows = 38.0 + 28.0;
    let row = |index: f32| rows + index * LINE + 10.0;
    let (plus_left, plus_right) = (22.0 + 9.0, right + 22.0 + 9.0);

    let anchors = comment_on(&opened, cx, (right + 200.0, row(1.0)), (plus_right, row(1.0)));
    assert_eq!(anchors, [ThreadAnchor::line("a.ts", Side::New, 2)]);

    let anchors = comment_on(&opened, cx, (200.0, row(1.0)), (plus_left, row(1.0)));
    assert_eq!(anchors.len(), 2);
    assert_eq!(anchors[1], ThreadAnchor::line("a.ts", Side::Old, 2));

    let empty_left = comment_on(&opened, cx, (200.0, row(2.0)), (plus_left, row(2.0)));
    assert_eq!(empty_left.len(), 2, "the second addition has nothing across from it");

    let anchors = comment_on(&opened, cx, (200.0, row(0.0)), (plus_left, row(0.0)));
    assert_eq!(
        anchors[2],
        ThreadAnchor::line("a.ts", Side::New, 1),
        "a context line is commented on its new side from either half"
    );
}

#[gpui::test]
fn toggling_the_mode_measures_the_threads_again_at_the_new_width(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    let id = opened.session.update(cx, |session, cx| {
        session.create_thread(ThreadAnchor::line("a.ts", Side::New, 2), cx)
    });
    cx.run_until_parked();
    let essay = "a long comment that wraps across many lines of the card ".repeat(30);
    opened.session.update(cx, |session, cx| session.post(id, &essay, cx));
    cx.run_until_parked();
    draw(&opened.pane, cx);
    let anchor = ThreadAnchor::line("a.ts", Side::New, 2);
    let unified = row_height(&opened, cx, id);
    assert_eq!(
        unified,
        frame_of(&opened, cx, &anchor).row_height(card_height(&opened, cx, id, &anchor))
    );

    opened.pane.update(cx, |pane, cx| pane.set_mode(DiffMode::Split, cx));
    draw(&opened.pane, cx);
    let split = row_height(&opened, cx, id);
    assert!(
        split > unified,
        "half the width wraps the comment more: {unified} then {split}"
    );
    assert_eq!(
        split,
        frame_of(&opened, cx, &anchor).row_height(card_height(&opened, cx, id, &anchor))
    );

    opened.pane.update(cx, |pane, cx| pane.set_mode(DiffMode::Unified, cx));
    draw(&opened.pane, cx);
    assert_eq!(row_height(&opened, cx, id), unified);
}

#[gpui::test]
fn comment_counts_follow_the_threads(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    run_review(&opened, cx);
    let counts = opened.session.read_with(cx, |session, _| session.comment_counts());
    assert_eq!(counts.get("a.ts"), Some(&1));
    assert_eq!(counts.get("b.ts"), Some(&1));
    assert_eq!(counts.get(""), Some(&2));
}

#[gpui::test]
fn the_session_learns_the_files_and_the_top_file(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    draw(&opened.pane, cx);
    opened.session.read_with(cx, |session, _| {
        assert_eq!(session.file_count(), 3);
        assert_eq!(session.host().visible_path.as_deref(), Some("a.ts"));
    });
    opened.pane.update(cx, |pane, cx| pane.reveal("b.ts", cx));
    opened.session.read_with(cx, |session, _| {
        assert_eq!(session.host().visible_path.as_deref(), Some("b.ts"))
    });
    opened.pane.update(cx, |pane, cx| pane.set_files(Vec::new(), cx));
    opened.session.read_with(cx, |session, _| {
        assert_eq!(session.file_count(), 0);
        assert_eq!(session.host().visible_path, None);
    });
}

#[gpui::test]
fn a_streamed_reply_grows_its_row_and_the_content_in_view_stays_put(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    let id = opened.session.update(cx, |session, cx| {
        session.create_thread(ThreadAnchor::line("a.ts", Side::New, 1), cx)
    });
    cx.run_until_parked();
    draw(&opened.pane, cx);
    opened
        .session
        .update(cx, |session, cx| session.post(id, "@claude why?", cx));
    cx.run_until_parked();
    draw(&opened.pane, cx);

    opened.pane.update(cx, |pane, cx| pane.reveal("b.ts", cx));
    opened.pane.update(cx, |pane, cx| pane.scroll_by(75.0, 0.0, cx));
    draw(&opened.pane, cx);
    let view = |cx: &mut VisualTestContext| {
        opened.pane.read_with(cx, |pane, _| {
            let state = pane.state();
            let row = state.tops.row_at(state.scroll()).unwrap();
            (row, state.scroll() - state.tops.top(row), state.scroll())
        })
    };
    let (row, offset, scroll) = view(cx);
    let height = row_height(&opened, cx, id);

    let mut previous = height;
    for chunk in 0..8 {
        opened.replier.send(&format!(
            "{chunk}: {}",
            "a long answer that wraps across lines ".repeat(5)
        ));
        cx.run_until_parked();
        let now = row_height(&opened, cx, id);
        assert!(now >= previous);
        previous = now;
        let (_, offset_now, scroll_now) = view(cx);
        assert_eq!(offset_now, offset, "the row under the viewport top does not move");
        assert_eq!(
            scroll_now - scroll,
            now - height,
            "the scroll follows the thread's growth"
        );
    }
    assert!(
        previous > height + 2.0 * metrics::LINE,
        "the reply made the row taller: {height} to {previous}"
    );
    assert_eq!(view(cx).0, row, "still the same row at the top");
}

#[gpui::test]
fn a_file_comment_opens_its_composer_in_view(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    opened.pane.update(cx, |pane, cx| pane.reveal("c.ts", cx));
    cx.run_until_parked();
    draw(&opened.pane, cx);
    let id = opened.pane.update_in(cx, |pane, window, cx| {
        let created = opened
            .session
            .update(cx, |session, cx| session.comment_on_visible_file(cx))
            .expect("a visible file");
        pane.focus_thread(created, window, cx);
        created
    });
    cx.run_until_parked();
    draw(&opened.pane, cx);
    opened.pane.read_with(cx, |pane, _| {
        let state = pane.state();
        let top = state.thread_top(id).expect("the thread is placed");
        assert!(
            top >= state.scroll() && top < state.scroll() + 600.0,
            "{top} vs {}",
            state.scroll()
        );
    });
    assert_eq!(
        row_height(&opened, cx, id),
        ThreadFrame::of(&ThreadAnchor::file("c.ts"), None).row_height(card_height(
            &opened,
            cx,
            id,
            &ThreadAnchor::file("c.ts")
        ))
    );
}

#[gpui::test]
fn the_plus_on_a_hovered_line_starts_a_comment_on_it(cx: &mut TestAppContext) {
    let (opened, cx) = open(cx);
    draw(&opened.pane, cx);
    let line = point(px(300.0), px(38.0 + 28.0 + 30.0));
    cx.simulate_mouse_move(line, None, Modifiers::default());
    draw(&opened.pane, cx);
    cx.simulate_click(point(px(70.0), px(38.0 + 28.0 + 20.0 + 10.0)), Modifiers::default());
    cx.run_until_parked();
    draw(&opened.pane, cx);
    let threads: Vec<_> = opened
        .session
        .read_with(cx, |session, _| session.threads().cloned().collect());
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].anchor, ThreadAnchor::line("a.ts", Side::New, 2));
}
