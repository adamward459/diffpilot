#![cfg(feature = "headless-tests")]

use diffpilot_core::changes::FileStatus;
use diffpilot_core::diff::{DiffLine, FileDiff, Hunk, LineKind};
use diffpilot_diff_pane::{DiffPane, DiffPaneEvent};
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

fn open(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<DiffPane>,
    &mut VisualTestContext,
    Rc<RefCell<Vec<DiffPaneEvent>>>,
) {
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
    pane.update(cx, |pane, cx| pane.set_dark(Some(true), cx));
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
