use crate::common::{finding, result};
use diffpilot::testing::{
    click, fake_claude, fixture, open_repo, open_repo_in, press, shows, type_text, wait_until_shows,
};
use diffpilot_core::review::Severity;
use diffpilot_core::settings::Settings;
use diffpilot_core::threads::{CLAUDE_NAME, USER_NAME};
use diffpilot_core::viewed::ViewedStore;
use gpui::TestAppContext;
use std::sync::Arc;

#[gpui::test]
fn a_comment_a_review_and_a_reply_end_up_as_threads_in_the_pane(app: &mut TestAppContext) {
    let dir = fixture();
    let fake = fake_claude(app);
    fake.review_with(&result(
        "Fine overall.",
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

    click(cx, "sidebar:row:docs/committed.md");
    click(cx, "pane:comment-on-file:docs/committed.md");
    type_text(cx, "Why this file?");
    press(cx, "cmd-enter");
    assert!(
        shows(cx, &format!("comment:{USER_NAME}:Why this file?")),
        "the pane shows the comment"
    );
    assert!(shows(cx, "jump-bar:threads:1"));
    assert!(shows(cx, "sidebar:comments:docs/committed.md:1"));

    click(cx, "review:open");
    click(cx, "review:start");
    wait_until_shows(cx, "review:done");
    assert_eq!(fake.launches(), 1);
    assert!(
        shows(cx, "jump-bar:threads:4"),
        "the review added a summary and two findings"
    );
    assert!(shows(cx, "sidebar:comments:base.txt:1"));
    assert!(shows(cx, "sidebar:comments:src/staged.rs:1"));
    assert!(
        shows(cx, "sidebar:comments:docs/committed.md:1"),
        "the review keeps the comment"
    );

    click(cx, "sidebar:row:src/staged.rs");
    assert!(shows(cx, "finding:Name"), "the pane shows the finding on the file");
    click(cx, "sidebar:row:base.txt");
    assert!(shows(cx, "finding:Edited"), "the pane shows the finding on the line");

    fake.hold();
    fake.end_without_review();
    click(cx, "thread:reply:Edited");
    type_text(cx, "@claude is this really needed?");
    press(cx, "cmd-enter");
    assert!(shows(
        cx,
        &format!("comment:{USER_NAME}:@claude is this really needed?")
    ));
    assert!(
        shows(cx, &format!("comment:{CLAUDE_NAME}:")),
        "the thread shows that Claude is on it"
    );
    fake.wait_for_launches(cx, 2);
    wait_until_shows(cx, "claude:status:Reply · running a command");

    let listed = fake.call_mcp_tool("list_threads", serde_json::json!({}));
    let threads = listed["result"]["threads"].as_array().expect("the run's threads");
    assert_eq!(threads.len(), 1, "a reply run sees only its thread: {listed}");
    assert_eq!(threads[0]["title"], "Edited");
    let answer = fake.call_mcp_tool(
        "reply",
        serde_json::json!({ "threadId": threads[0]["id"], "body": "Yes, the line changed." }),
    );
    assert_eq!(answer["ok"], true, "{answer}");
    wait_until_shows(cx, &format!("comment:{CLAUDE_NAME}:Yes, the line changed."));
    fake.finish();
    wait_until_shows(cx, "claude:status:Reply finished · 0 comments · 3 tool calls");
}

#[gpui::test]
fn comments_a_running_review_writes_become_threads_and_the_next_review_replaces_them(app: &mut TestAppContext) {
    let dir = fixture();
    let fake = fake_claude(app);
    fake.hold();
    let (_, cx) = open_repo_in(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
        fake.store(),
    );

    click(cx, "sidebar:row:docs/committed.md");
    click(cx, "pane:comment-on-file:docs/committed.md");
    type_text(cx, "Why this file?");
    press(cx, "cmd-enter");
    assert!(shows(cx, "jump-bar:threads:1"));

    click(cx, "review:open");
    click(cx, "review:start");
    fake.wait_for_launches(cx, 1);
    let comment = fake.call_mcp_tool(
        "add_comment",
        serde_json::json!({
            "file": "base.txt", "startLine": 2, "severity": "must-fix", "title": "Edited", "body": "Check this line.",
        }),
    );
    assert_eq!(comment["ok"], true, "{comment}");
    wait_until_shows(cx, "jump-bar:threads:2");

    fake.review_with(&result(
        "",
        vec![
            finding("base.txt", Some(2), Severity::Nit, "Second"),
            finding("src/staged.rs", None, Severity::Nit, "Third"),
            finding("docs/committed.md", None, Severity::Question, "Fourth"),
        ],
    ));
    fake.finish();
    wait_until_shows(cx, "review:done");
    assert!(
        shows(cx, "jump-bar:threads:2"),
        "the comment of the first review stays until the run ends"
    );

    click(cx, "review:open");
    click(cx, "claude-area:run-again");
    wait_until_shows(cx, "jump-bar:threads:4");
}

#[gpui::test]
fn a_review_that_only_comments_replaces_the_old_findings(app: &mut TestAppContext) {
    let dir = fixture();
    let fake = fake_claude(app);
    fake.review_with(&result(
        "",
        vec![
            finding("base.txt", Some(2), Severity::Nit, "First"),
            finding("src/staged.rs", None, Severity::Nit, "Second"),
        ],
    ));
    fake.finish();
    let (_, cx) = open_repo_in(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
        fake.store(),
    );
    click(cx, "review:open");
    click(cx, "review:start");
    wait_until_shows(cx, "jump-bar:threads:2");
    wait_until_shows(cx, "review:done");

    fake.hold();
    fake.end_without_review();
    click(cx, "review:open");
    click(cx, "claude-area:run-again");
    fake.wait_for_launches(cx, 2);
    let comment = fake.call_mcp_tool(
        "add_comment",
        serde_json::json!({
            "file": "base.txt", "startLine": 2, "severity": "must-fix", "title": "Edited", "body": "Check this line.",
        }),
    );
    assert_eq!(comment["ok"], true, "{comment}");
    wait_until_shows(cx, "jump-bar:threads:3");
    fake.finish();
    wait_until_shows(cx, "jump-bar:threads:1");
}

#[gpui::test]
fn the_thread_arrows_step_through_the_threads(app: &mut TestAppContext) {
    let dir = fixture();
    let fake = fake_claude(app);
    fake.review_with(&result(
        "",
        vec![
            finding("base.txt", Some(2), Severity::Nit, "First"),
            finding("src/staged.rs", None, Severity::Nit, "Second"),
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
    wait_until_shows(cx, "jump-bar:threads:2");

    assert!(!shows(cx, "jump-bar:path:src/staged.rs"));
    click(cx, "jump-bar:thread-next");
    assert!(
        shows(cx, "jump-bar:path:src/staged.rs"),
        "the first thread's file is in view"
    );
}

#[gpui::test]
fn the_previous_thread_arrow_reaches_the_review_summary(app: &mut TestAppContext) {
    let dir = fixture();
    let fake = fake_claude(app);
    fake.review_with(&result(
        "Fine overall.",
        vec![finding("src/staged.rs", Some(280), Severity::Nit, "Late")],
    ));
    fake.finish();
    let (_, cx) = open_repo(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
    );
    click(cx, "sidebar:row:src/staged.rs");
    click(cx, "review:open");
    click(cx, "review:start");
    wait_until_shows(cx, "review:done");
    wait_until_shows(cx, "jump-bar:threads:2");

    click(cx, "jump-bar:thread-next");
    assert!(shows(cx, "finding:Late"));
    let summary = format!("comment:{CLAUDE_NAME}:Fine overall.");
    assert!(!shows(cx, &summary), "the summary is out of view");
    click(cx, "jump-bar:thread-previous");
    assert!(shows(cx, &summary), "the previous arrow scrolls up to the summary");
}

#[gpui::test]
fn picking_an_effort_in_the_review_popover_changes_the_model(app: &mut TestAppContext) {
    let dir = fixture();
    let (_, cx) = open_repo(
        app,
        dir.path(),
        Arc::new(Settings::in_memory()),
        Arc::new(ViewedStore::in_memory()),
    );

    click(cx, "review:open");
    click(cx, "review:model:Opus · High");
    click(cx, "review-model:Low");
    assert!(
        shows(cx, "review:model:Opus · Low"),
        "the button shows the picked effort"
    );

    click(cx, "review:model:Opus · Low");
    click(cx, "review-model:Sonnet");
    assert!(
        shows(cx, "review:model:Sonnet · Low"),
        "the review popover stays open, so a second pick works"
    );
}
