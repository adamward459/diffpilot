use crate::common::{assert_card_inside_its_row, comment_on_file, finding, open_on, result};
use diffpilot::review::pill::RESULT_SECONDS;
use diffpilot::testing::{
    FakeClaude, advance_clock, click, fake_claude, fixture, open_repo, press, shows, type_text, wait_until_shows,
};
use diffpilot_core::policy::EditPermission;
use diffpilot_core::review::Severity;
use diffpilot_core::settings::Settings;
use diffpilot_core::threads::{CLAUDE_NAME, USER_NAME};
use diffpilot_core::viewed::ViewedStore;
use gpui::{TestAppContext, VisualTestContext};
use serde_json::json;
use std::sync::Arc;

fn open_fix(app: &mut TestAppContext) -> (tempfile::TempDir, &'static mut VisualTestContext) {
    let dir = fixture();
    let settings = Arc::new(Settings::in_memory());
    settings.set_edit_permission(EditPermission::DontAsk);
    let (_, cx) = open_repo(app, dir.path(), settings, Arc::new(ViewedStore::in_memory()));
    (dir, cx)
}

fn start_fix(cx: &mut VisualTestContext, claude: &FakeClaude) -> serde_json::Value {
    click(cx, "fix:open");
    click(cx, "fix:send");
    claude.wait_for_launches(cx, 1);
    let listed = claude.call_mcp_tool("list_threads", json!({}));
    listed["result"]["threads"][0]["id"].clone()
}

#[gpui::test]
fn a_nit_starts_collapsed_to_its_title_and_the_arrow_opens_it(app: &mut TestAppContext) {
    let dir = fixture();
    let fake = fake_claude(app);
    fake.review_with(&result(
        "",
        vec![
            finding("base.txt", Some(2), Severity::MustFix, "Edited"),
            finding("src/staged.rs", None, Severity::Nit, "Name"),
        ],
    ));
    fake.finish();
    let (_, cx) = open_repo(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
    );
    click(cx, "review:open");
    click(cx, "review:start");
    wait_until_shows(cx, "review:done");

    click(cx, "sidebar:row:base.txt");
    assert!(shows(cx, "finding:Edited"));
    assert!(
        shows(cx, &format!("comment:{CLAUDE_NAME}:Edited body")),
        "a must-fix shows its text"
    );
    click(cx, "sidebar:row:src/staged.rs");
    assert!(shows(cx, "finding:Name"));
    assert!(
        !shows(cx, &format!("comment:{CLAUDE_NAME}:Name body")),
        "a nit shows only its title"
    );
    click(cx, "thread:fold:Name");
    assert!(shows(cx, &format!("comment:{CLAUDE_NAME}:Name body")));
}

#[gpui::test]
fn cancel_on_a_new_comment_removes_it_instead_of_leaving_an_empty_thread(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    let dir = fixture();
    let cx = open_on(app, dir.path(), fake.store());
    click(cx, "sidebar:row:docs/committed.md");
    click(cx, "pane:comment-on-file:docs/committed.md");
    assert!(shows(cx, "thread:cancel:docs/committed.md"));
    click(cx, "thread:cancel:docs/committed.md");

    let again = open_on(app, dir.path(), fake.store());
    click(again, "sidebar:row:docs/committed.md");
    assert!(
        !shows(again, "thread:cancel:docs/committed.md"),
        "no empty comment box comes back"
    );
    click(again, "toolbar:comments");
    click(again, "comments:status:resolved");
    assert!(!shows(again, "comments:row:"), "and none sits in the resolved list");
}

#[gpui::test]
fn a_thread_claude_addressed_shows_it_and_reopening_makes_it_fixable_again(app: &mut TestAppContext) {
    let claude = fake_claude(app);
    let (_dir, cx) = open_fix(app);
    comment_on_file(cx, "docs/committed.md", "Rename this.");
    let id = start_fix(cx, &claude);
    let answer = claude.call_mcp_tool("resolve", json!({ "threadId": id }));
    assert_eq!(answer["ok"], true, "{answer}");
    claude.finish();
    wait_until_shows(cx, "fix:result");
    assert!(shows(cx, "thread:status:Addressed by Claude"));

    advance_clock(cx, RESULT_SECONDS + 1);
    comment_on_file(cx, "src/staged.rs", "Rename that.");
    assert!(
        !shows(cx, "fix:ready:2"),
        "an addressed thread is not offered to Claude again"
    );
    click(cx, "sidebar:row:docs/committed.md");
    click(cx, "thread:reopen:docs/committed.md");
    wait_until_shows(cx, "fix:ready:2");
}

#[gpui::test]
fn a_thread_claude_dismissed_keeps_its_card_with_the_reason_and_cannot_be_reopened(app: &mut TestAppContext) {
    let claude = fake_claude(app);
    let (_dir, cx) = open_fix(app);
    comment_on_file(cx, "docs/committed.md", "Is this needed?");
    let id = start_fix(cx, &claude);
    let answer = claude.call_mcp_tool("dismiss", json!({ "threadId": id, "reason": "It is needed." }));
    assert_eq!(answer["ok"], true, "{answer}");
    claude.finish();
    wait_until_shows(cx, "fix:result");

    assert!(shows(cx, "thread:status:Dismissed by Claude"));
    assert!(shows(cx, &format!("comment:{CLAUDE_NAME}:It is needed.")));
    assert!(shows(cx, &format!("comment:{USER_NAME}:Is this needed?")));
    assert!(!shows(cx, "thread:reopen:docs/committed.md"));
}

#[gpui::test]
fn a_card_stays_inside_its_row_when_a_review_lands_and_when_a_long_reply_arrives(app: &mut TestAppContext) {
    let dir = fixture();
    let fake = fake_claude(app);
    fake.review_with(&result(
        "Fine overall.",
        vec![finding("base.txt", Some(2), Severity::MustFix, "Edited")],
    ));
    fake.finish();
    let (_, cx) = open_repo(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
    );
    click(cx, "review:open");
    click(cx, "review:start");
    wait_until_shows(cx, "review:done");
    click(cx, "sidebar:row:base.txt");
    assert!(shows(cx, "finding:Edited"));
    assert_card_inside_its_row(cx, "Edited");

    fake.hold();
    fake.end_without_review();
    click(cx, "thread:reply:Edited");
    type_text(cx, "@claude is this really needed?");
    press(cx, "cmd-enter");
    fake.wait_for_launches(cx, 2);
    let listed = fake.call_mcp_tool("list_threads", json!({}));
    let reply = ["a long answer that wraps over many lines"; 12].join(", ");
    let answer = fake.call_mcp_tool(
        "reply",
        json!({ "threadId": listed["result"]["threads"][0]["id"], "body": reply }),
    );
    assert_eq!(answer["ok"], true, "{answer}");
    wait_until_shows(cx, &format!("comment:{CLAUDE_NAME}:{reply}"));
    assert_card_inside_its_row(cx, "Edited");
    fake.finish();
}
