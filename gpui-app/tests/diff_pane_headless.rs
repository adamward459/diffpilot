use diffpilot::diff_pane::rows::RowKind;
use diffpilot::diff_pane::{DiffPane, DiffPaneEvent};
use diffpilot_core::changes::FileStatus;
use diffpilot_core::diff::{DiffLine, DiffMode, FileDiff, Hunk, LineKind};
use gpui::{ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, point, px, size};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

fn file(path: &str, lines: usize) -> FileDiff {
    let mut diff = FileDiff::new(path, FileStatus::Modified);
    let lines: Vec<DiffLine> = (1..=lines as u32)
        .map(|number| DiffLine {
            kind: LineKind::Added,
            old: None,
            new: Some(number),
            text: format!("const value{number} = {number}; // {path}"),
        })
        .collect();
    diff.additions = lines.len() as u32;
    diff.hunks.push(Hunk {
        header: "@@ -0,0 +1 @@".into(),
        lines,
    });
    diff
}

fn files() -> Vec<FileDiff> {
    ["a.ts", "b.ts", "c.ts", "d.ts"]
        .iter()
        .map(|path| file(path, 40))
        .collect()
}

fn changed(path: &str, status: FileStatus, spec: &str) -> FileDiff {
    let (mut old, mut new) = (0, 0);
    let lines: Vec<DiffLine> = spec
        .chars()
        .map(|kind| {
            let (kind, number) = match kind {
                'd' => (LineKind::Deleted, &mut old),
                'a' => (LineKind::Added, &mut new),
                _ => (LineKind::Context, &mut old),
            };
            *number += 1;
            let position = *number;
            if kind == LineKind::Context {
                new += 1;
            }
            DiffLine {
                kind,
                old: (kind != LineKind::Added).then_some(position),
                new: (kind != LineKind::Deleted).then_some(if kind == LineKind::Context { new } else { position }),
                text: format!(
                    "let {path}_{position} = {position}; // {}",
                    "words ".repeat(position as usize % 9)
                ),
            }
        })
        .collect();
    let mut diff = FileDiff::new(path, status);
    diff.additions = lines.iter().filter(|line| line.kind == LineKind::Added).count() as u32;
    diff.deletions = lines.iter().filter(|line| line.kind == LineKind::Deleted).count() as u32;
    diff.hunks.push(Hunk {
        header: "@@ -1 +1 @@".into(),
        lines,
    });
    diff
}

fn open(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<DiffPane>,
    &mut VisualTestContext,
    Rc<RefCell<Vec<DiffPaneEvent>>>,
) {
    cx.update(diffpilot_ui_kit::init);
    let (pane, cx) = cx.add_window_view(DiffPane::new);
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&pane, move |_, event: &DiffPaneEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    (pane, cx, events)
}

fn draw(pane: &gpui::Entity<DiffPane>, cx: &mut VisualTestContext) {
    let pane = pane.clone();
    cx.draw(point(px(0.0), px(0.0)), size(px(900.0), px(600.0)), move |_, _| pane);
}

#[gpui::test]
fn reports_the_top_file_when_files_arrive_and_when_one_is_revealed(cx: &mut TestAppContext) {
    let (pane, cx, events) = open(cx);
    pane.update(cx, |pane, cx| pane.set_files(files(), cx));
    pane.update(cx, |pane, cx| pane.reveal("c.ts", cx));
    pane.update(cx, |pane, cx| pane.reveal("missing.ts", cx));
    assert_eq!(
        *events.borrow(),
        [
            DiffPaneEvent::VisibleFile("a.ts".into()),
            DiffPaneEvent::VisibleFile("c.ts".into())
        ]
    );
}

#[gpui::test]
fn draws_every_row_kind_in_both_wrap_modes(cx: &mut TestAppContext) {
    let (pane, cx, _) = open(cx);
    let mut binary = FileDiff::new("logo.png", FileStatus::Added);
    binary.is_binary = true;
    let mut all = files();
    all.push(binary);
    all.push(file("pnpm-lock.yaml", 3));
    pane.update(cx, |pane, cx| {
        pane.set_files(all, cx);
        pane.set_viewed(HashSet::from(["b.ts".to_string()]), cx);
    });
    draw(&pane, cx);
    pane.update(cx, |pane, cx| pane.scroll_to(200.0, cx));
    draw(&pane, cx);
    pane.update(cx, |pane, cx| pane.set_wrap(false, cx));
    draw(&pane, cx);
    cx.update(|_, cx| diffpilot_ui_kit::theme::set_dark(true, None, cx));
    draw(&pane, cx);
    pane.update(cx, |pane, cx| pane.reveal("pnpm-lock.yaml", cx));
    draw(&pane, cx);
    let (collapsed, rows) = pane.read_with(cx, |pane, _| {
        (pane.state().collapsed.len(), pane.state().rows.rows.len())
    });
    assert_eq!(collapsed, 1);
    assert!(rows > 100);
}

#[gpui::test]
fn the_scroll_wheel_moves_the_pane_and_changes_the_top_file(cx: &mut TestAppContext) {
    let (pane, cx, events) = open(cx);
    pane.update(cx, |pane, cx| pane.set_files(files(), cx));
    draw(&pane, cx);
    let before = pane.read_with(cx, |pane, _| pane.state().scroll());
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(100.0), px(100.0)),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-1000.0))),
        ..Default::default()
    });
    draw(&pane, cx);
    let after = pane.read_with(cx, |pane, _| pane.state().scroll());
    assert_eq!(before, 0.0);
    assert!(after > 900.0, "scrolled to {after}");
    assert!(events.borrow().len() >= 2, "{:?}", events.borrow());
}

#[gpui::test]
fn highlights_arrive_from_the_background_and_apply(cx: &mut TestAppContext) {
    let (pane, cx, _) = open(cx);
    pane.update(cx, |pane, cx| pane.set_files(files(), cx));
    cx.run_until_parked();
    let (ready, busy) = pane.read_with(cx, |pane, _| (pane.highlighted_files(), pane.is_highlighting()));
    assert!(ready >= 3, "{ready} files highlighted");
    assert!(!busy);
}

fn split_files() -> Vec<FileDiff> {
    let mut renamed = changed("moved.ts", FileStatus::Renamed, "cddaacdaaa");
    renamed.old_path = Some("old/moved.ts".into());
    let mut binary = FileDiff::new("logo.png", FileStatus::Added);
    binary.is_binary = true;
    let mut untouched_rename = FileDiff::new("same.txt", FileStatus::Renamed);
    untouched_rename.old_path = Some("was.txt".into());
    vec![
        changed("modified.ts", FileStatus::Modified, "cccdddaaaccddaacdadaccc"),
        changed("added.ts", FileStatus::Added, &"a".repeat(30)),
        changed("deleted.ts", FileStatus::Deleted, &"d".repeat(30)),
        renamed,
        binary,
        untouched_rename,
        changed("pnpm-lock.yaml", FileStatus::Modified, "cdac"),
        changed("viewed.ts", FileStatus::Modified, "cdda"),
    ]
}

fn pair_rows(pane: &gpui::Entity<DiffPane>, cx: &mut VisualTestContext) -> usize {
    pane.read_with(cx, |pane, _| {
        pane.state()
            .rows
            .rows
            .iter()
            .filter(|row| matches!(row.kind, RowKind::Pair { .. }))
            .count()
    })
}

fn resize(cx: &mut VisualTestContext, width: f32) {
    cx.simulate_resize(size(px(width), px(600.0)));
    cx.run_until_parked();
}

#[gpui::test]
fn draws_split_rows_of_every_kind_and_toggles_the_mode(cx: &mut TestAppContext) {
    let (pane, cx, events) = open(cx);
    pane.update(cx, |pane, cx| {
        pane.set_files(split_files(), cx);
        pane.set_viewed(HashSet::from(["viewed.ts".to_string()]), cx);
        pane.set_mode(DiffMode::Split, cx);
    });
    draw(&pane, cx);
    assert!(pair_rows(&pane, cx) > 60);
    for offset in [0.0, 300.0, 700.0, 5000.0] {
        pane.update(cx, |pane, cx| pane.scroll_to(offset, cx));
        draw(&pane, cx);
    }
    pane.update(cx, |pane, cx| pane.set_wrap(false, cx));
    draw(&pane, cx);
    pane.update(cx, |pane, cx| pane.scroll_by(0.0, 300.0, cx));
    draw(&pane, cx);
    cx.update(|_, cx| diffpilot_ui_kit::theme::set_dark(true, None, cx));
    draw(&pane, cx);
    pane.update(cx, |pane, cx| pane.reveal("pnpm-lock.yaml", cx));
    draw(&pane, cx);
    pane.update(cx, |pane, cx| pane.set_wrap(true, cx));
    draw(&pane, cx);

    pane.update(cx, |pane, cx| pane.set_mode(DiffMode::Unified, cx));
    draw(&pane, cx);
    assert_eq!(pair_rows(&pane, cx), 0);
    pane.update(cx, |pane, cx| pane.set_mode(DiffMode::Split, cx));
    draw(&pane, cx);
    assert!(pair_rows(&pane, cx) > 60);

    let shown: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            DiffPaneEvent::ShownMode(mode) => Some(*mode),
            _ => None,
        })
        .collect();
    assert_eq!(shown, [DiffMode::Split, DiffMode::Unified, DiffMode::Split]);
}

#[gpui::test]
fn a_pane_too_narrow_for_two_columns_draws_unified_rows_and_says_so(cx: &mut TestAppContext) {
    let (pane, cx, events) = open(cx);
    pane.update(cx, |pane, cx| {
        pane.set_files(split_files(), cx);
        pane.set_mode(DiffMode::Split, cx);
    });
    resize(cx, 1200.0);
    assert!(pair_rows(&pane, cx) > 0);
    resize(cx, 700.0);
    assert_eq!(pair_rows(&pane, cx), 0);
    pane.read_with(cx, |pane, _| {
        assert_eq!(pane.state().mode(), DiffMode::Split);
        assert_eq!(pane.state().shown_mode(), DiffMode::Unified);
    });
    resize(cx, 1200.0);
    assert!(pair_rows(&pane, cx) > 0);
    let shown: Vec<_> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            DiffPaneEvent::ShownMode(mode) => Some(*mode),
            _ => None,
        })
        .collect();
    assert_eq!(shown, [DiffMode::Split, DiffMode::Unified, DiffMode::Split]);
}

#[gpui::test]
fn the_top_line_stays_put_when_the_mode_changes_under_a_live_window(cx: &mut TestAppContext) {
    let (pane, cx, _) = open(cx);
    pane.update(cx, |pane, cx| pane.set_files(split_files(), cx));
    draw(&pane, cx);
    pane.update(cx, |pane, cx| pane.scroll_to(900.0, cx));
    draw(&pane, cx);
    let top = |cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, _| {
            let state = pane.state();
            let row = state.rows.rows[state.tops.row_at(state.scroll()).unwrap()];
            (state.visible_path().map(str::to_owned), row.hunk(), row.kind)
        })
    };
    let (path, hunk, kind) = top(cx);
    let RowKind::Line { line, .. } = kind else {
        panic!("a line is at the top: {kind:?}")
    };
    pane.update(cx, |pane, cx| pane.set_mode(DiffMode::Split, cx));
    draw(&pane, cx);
    let (split_path, split_hunk, split_kind) = top(cx);
    assert_eq!((split_path, split_hunk), (path, hunk));
    let shown = match split_kind {
        RowKind::Pair { left, right, .. } => left == Some(line) || right == Some(line),
        _ => false,
    };
    assert!(shown, "{split_kind:?} shows line {line}");
}
