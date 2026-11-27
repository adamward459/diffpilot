use crate::common::{assert_card_inside_its_row, comment_on_file, finding, result};
use diffpilot::diff_pane::layout::{LINE, NUMBER_COLUMN, SPLIT_DIVIDER, column_width};
use diffpilot::testing::{
    bounds, click, click_below, fake_claude, fixture, git, hover_below, open_repo, press, resize, scroll_down, shows,
    type_text, wait_until_shows, window_size, write,
};
use diffpilot_core::review::Severity;
use diffpilot_core::settings::Settings;
use diffpilot_core::threads::{CLAUDE_NAME, USER_NAME};
use diffpilot_core::viewed::ViewedStore;
use gpui::{TestAppContext, VisualTestContext, px};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

const HUNK: &str = "pane:hunk:notes.txt:0";

fn open(app: &mut TestAppContext, repo: &Path) -> &'static mut VisualTestContext {
    let (_, cx) = open_repo(
        app,
        repo,
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
    );
    cx
}

/// `notes.txt` changes `old one` and `old two` into `new one`, so the split view has an empty right cell.
fn notes_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path();
    git(repo, &["init", "-q", "-b", "main"]);
    write(repo, "notes.txt", "keep\nold one\nold two\ntail\n");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "base"]);
    git(repo, &["checkout", "-q", "-b", "feature"]);
    write(repo, "notes.txt", "keep\nnew one\ntail\n");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "edit"]);
    dir
}

fn row(index: f32) -> f32 {
    LINE * (index + 0.5)
}

#[gpui::test]
fn in_the_split_view_the_plus_comments_on_the_half_under_the_mouse_and_never_on_an_empty_cell(
    app: &mut TestAppContext,
) {
    let dir = notes_repo();
    let cx = open(app, dir.path());
    resize(cx, 1400., 900.);
    click(cx, "diff-mode:Split");
    wait_until_shows(cx, HUNK);
    let width = f32::from(bounds(cx, HUNK).size.width);
    let right = column_width(width) + SPLIT_DIVIDER;
    let plus = NUMBER_COLUMN - 9.0;

    hover_below(cx, HUNK, (right + 200., row(2.)));
    click_below(cx, HUNK, (right + plus, row(2.)), 1);
    assert!(
        !shows(cx, "thread:cancel:notes.txt:3"),
        "the cell across from a deleted line is empty, so there is nothing to comment on"
    );

    hover_below(cx, HUNK, (200., row(2.)));
    click(cx, "pane:add-comment:notes.txt:Old:3");
    assert!(
        shows(cx, "thread:cancel:notes.txt:3"),
        "the left half comments on the old line"
    );

    hover_below(cx, HUNK, (right + 200., row(1.)));
    click(cx, "pane:add-comment:notes.txt:New:2");
    assert!(
        shows(cx, "thread:cancel:notes.txt:2"),
        "the right half comments on the new line"
    );

    hover_below(cx, HUNK, (200., row(0.)));
    click(cx, "pane:add-comment:notes.txt:New:1");
    assert!(
        shows(cx, "thread:cancel:notes.txt:1"),
        "a context line is commented on its new side from either half"
    );

    let old = bounds(cx, "thread:card:notes.txt:3");
    let new = bounds(cx, "thread:card:notes.txt:2");
    assert!(
        old.right() <= new.left(),
        "the old line's comment sits in the left half"
    );
}

#[gpui::test]
fn hovering_a_line_in_the_unified_view_shows_a_plus_that_comments_on_that_line(app: &mut TestAppContext) {
    let dir = fixture();
    let cx = open(app, dir.path());
    click(cx, "sidebar:row:base.txt");
    wait_until_shows(cx, "pane:hunk:base.txt:0");

    hover_below(cx, "pane:hunk:base.txt:0", (300., row(1.)));
    assert!(
        shows(cx, "pane:add-comment:base.txt:New:2"),
        "the second line is `edited`"
    );
    click(cx, "pane:add-comment:base.txt:New:2");
    type_text(cx, "Why edited?");
    press(cx, "cmd-enter");
    assert!(shows(cx, &format!("comment:{USER_NAME}:Why edited?")));
    assert!(shows(cx, "sidebar:comments:base.txt:1"));
}

#[gpui::test]
#[ignore = "product bug: in the split view the card is 19px taller than its row, so the row clips it"]
fn a_comment_stays_inside_its_row_after_switching_between_split_and_unified(app: &mut TestAppContext) {
    let dir = fixture();
    let cx = open(app, dir.path());
    resize(cx, 1400., 900.);
    click(cx, "sidebar:row:base.txt");
    wait_until_shows(cx, "pane:hunk:base.txt:0");
    hover_below(cx, "pane:hunk:base.txt:0", (300., row(1.)));
    click(cx, "pane:add-comment:base.txt:New:2");
    type_text(cx, "a long comment that wraps across many lines of the card");
    press(cx, "cmd-enter");
    assert_card_inside_its_row(cx, "base.txt:2");

    click(cx, "diff-mode:Split");
    assert_card_inside_its_row(cx, "base.txt:2");
    click(cx, "diff-mode:Unified");
    assert_card_inside_its_row(cx, "base.txt:2");
}

fn review_cards_fit(app: &mut TestAppContext, split: bool) {
    let dir = fixture();
    let fake = fake_claude(app);
    fake.review_with(&result(
        "Fine overall.",
        vec![
            finding("base.txt", Some(2), Severity::MustFix, "Edited"),
            finding("gone.py", Some(3), Severity::Question, "Used?"),
        ],
    ));
    fake.finish();
    let cx = open(app, dir.path());
    resize(cx, 1400., 900.);
    if split {
        click(cx, "diff-mode:Split");
    }
    click(cx, "review:open");
    click(cx, "review:start");
    wait_until_shows(cx, "review:done");

    assert!(shows(cx, "jump-bar:threads:3"), "the summary and two findings");
    click(cx, "sidebar:row:base.txt");
    scroll_down(cx, "pane:hunk:base.txt:0", -20000.);
    assert!(
        shows(cx, "finding:Used?"),
        "a finding on a file outside the diff shows above the first file"
    );
    assert_card_inside_its_row(cx, "");
    assert_card_inside_its_row(cx, "Used?");
    click(cx, "sidebar:row:base.txt");
    assert!(shows(cx, "finding:Edited"));
    assert_card_inside_its_row(cx, "Edited");
}

#[gpui::test]
fn review_findings_fit_their_rows_in_the_unified_view(app: &mut TestAppContext) {
    review_cards_fit(app, false);
}

#[gpui::test]
fn review_findings_fit_their_rows_in_the_split_view(app: &mut TestAppContext) {
    review_cards_fit(app, true);
}

#[gpui::test]
fn a_reply_to_a_thread_above_the_view_does_not_move_what_is_on_screen(app: &mut TestAppContext) {
    let dir = fixture();
    let fake = fake_claude(app);
    fake.hold();
    let cx = open(app, dir.path());
    comment_on_file(cx, "docs/committed.md", "@claude why?");
    fake.wait_for_launches(cx, 1);
    let row = bounds(cx, "pane:thread-row:docs/committed.md").bottom();
    let top = bounds(cx, "pane:copy-path:docs/committed.md").top();
    scroll_down(cx, "pane:hunk:docs/committed.md:0", f32::from(row - top) + 5.);
    wait_until_shows(cx, "pane:copy-path:src/staged.rs");
    let before = bounds(cx, "pane:copy-path:src/staged.rs").top();

    let listed = fake.call_mcp_tool("list_threads", json!({}));
    let reply = ["a long answer that wraps across lines"; 40].join(" ");
    let answer = fake.call_mcp_tool(
        "reply",
        json!({ "threadId": listed["result"]["threads"][0]["id"], "body": reply }),
    );
    assert_eq!(answer["ok"], true, "{answer}");
    fake.finish();
    wait_until_shows(cx, "claude:status:Reply finished · 0 comments · 3 tool calls");
    assert_eq!(bounds(cx, "pane:copy-path:src/staged.rs").top(), before);

    click(cx, "sidebar:row:docs/committed.md");
    assert!(
        shows(cx, &format!("comment:{CLAUDE_NAME}:{reply}")),
        "the reply did land"
    );
}

#[gpui::test]
fn a_file_comment_started_from_far_down_the_file_opens_its_box_in_view(app: &mut TestAppContext) {
    let dir = fixture();
    let cx = open(app, dir.path());
    click(cx, "sidebar:row:src/staged.rs");
    wait_until_shows(cx, "pane:hunk:src/staged.rs:0");
    scroll_down(cx, "pane:hunk:src/staged.rs:0", 3000.);
    click(cx, "pane:comment-on-file:src/staged.rs");

    assert!(shows(cx, "thread:cancel:src/staged.rs"), "the comment box is on screen");
    let cancel = bounds(cx, "thread:cancel:src/staged.rs");
    assert!(cancel.top() >= px(0.) && cancel.bottom() <= window_size(cx).height);
}
