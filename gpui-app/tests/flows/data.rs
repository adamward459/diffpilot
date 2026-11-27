use diffpilot::settings::Section;
use diffpilot::testing::{
    canned_review, click, draw, fake_claude, fixture_named, install_settings, open_settings, open_workspace, press,
    show_settings, shows, type_text, wait_until_shows,
};
use diffpilot_core::settings::Settings;
use diffpilot_core::threads::USER_NAME;
use gpui::{TestAppContext, VisualTestContext};
use std::path::Path;
use std::sync::Arc;

fn comment_on_file(cx: &mut VisualTestContext, path: &str, text: &str) {
    click(cx, &format!("sidebar:row:{path}"));
    click(cx, &format!("pane:comment-on-file:{path}"));
    type_text(cx, text);
    press(cx, "cmd-enter");
}

#[gpui::test]
fn deleting_a_branch_in_settings_clears_its_threads_and_new_comments_still_work(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    let store = fake.store();
    let (_dir, repo) = fixture_named("alpha");
    install_settings(app, Arc::new(Settings::in_memory()));
    let cx = open_workspace(app, &repo, Arc::new(Settings::in_memory()));
    comment_on_file(cx, "docs/committed.md", "First thought.");
    assert!(shows(cx, "jump-bar:threads:1"));
    let key = store.projects().unwrap()[0].repo.clone();

    show_settings(cx, Section::Data);
    wait_until_shows(cx, &format!("data:repo:{key}:1 thread · 1 comment"));
    click(cx, &format!("data:expand:{key}"));
    assert!(shows(cx, &format!("data:branch:{key}:feature:1 thread · 1 comment")));

    click(cx, &format!("data:delete:branch:{key}:feature"));
    assert!(shows(cx, "data:confirm"), "the page asks before it deletes");
    click(cx, "data:confirm");
    wait_until_shows(cx, "data:empty");
    press(cx, "escape");

    wait_until_shows(cx, "jump-bar:threads:0");
    comment_on_file(cx, "docs/committed.md", "Second thought.");
    assert!(shows(cx, &format!("comment:{USER_NAME}:Second thought.")));
    let branches = store.projects().unwrap().remove(0).branches;
    assert_eq!(branches.len(), 1);
    assert_eq!((branches[0].threads, branches[0].comments), (1, 1));
}

#[gpui::test]
fn cancel_keeps_the_data_and_a_missing_folder_is_marked(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    let store = fake.store();
    let (dir, repo) = fixture_named("alpha");
    install_settings(app, Arc::new(Settings::in_memory()));
    let cx = open_workspace(app, &repo, Arc::new(Settings::in_memory()));
    comment_on_file(cx, "docs/committed.md", "Keep me.");
    let key = store.projects().unwrap()[0].repo.clone();
    std::fs::remove_dir_all(dir.path()).unwrap();

    show_settings(cx, Section::Data);
    wait_until_shows(cx, &format!("data:missing:{key}"));
    click(cx, &format!("data:delete:repo:{key}"));
    click(cx, "data:cancel");
    assert!(shows(cx, "data:delete:all"), "cancel puts the Delete buttons back");
    assert_eq!(store.projects().unwrap()[0].repo, key, "nothing was deleted");
}

#[gpui::test]
fn deleting_a_repo_stops_its_running_review(app: &mut TestAppContext) {
    let fake = fake_claude(app);
    fake.review_with(&canned_review());
    fake.hold();
    let store = fake.store();
    let (_dir, repo) = fixture_named("alpha");
    install_settings(app, Arc::new(Settings::in_memory()));
    let cx = open_workspace(app, &repo, Arc::new(Settings::in_memory()));
    click(cx, "review:open");
    click(cx, "review:start");
    fake.wait_for_launches(cx, 1);
    let key = store.projects().unwrap()[0].repo.clone();
    assert!(fake.engine().active(Path::new(&key)).is_some());

    click(cx, "sidebar:row:base.txt");
    show_settings(cx, Section::Data);
    wait_until_shows(cx, &format!("data:expand:{key}"));
    click(cx, &format!("data:delete:repo:{key}"));
    click(cx, "data:confirm");
    wait_until_shows(cx, "data:empty");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while fake.engine().active(Path::new(&key)).is_some() {
        assert!(std::time::Instant::now() < deadline, "the run kept going");
        std::thread::sleep(std::time::Duration::from_millis(20));
        draw(cx);
    }
    assert!(
        store.projects().unwrap().is_empty(),
        "the stopped run wrote nothing back"
    );
}

#[gpui::test]
fn the_data_section_opens_from_the_section_list(app: &mut TestAppContext) {
    let _fake = fake_claude(app);
    let cx = open_settings(app, None, Arc::new(Settings::in_memory()), Section::Data);
    wait_until_shows(cx, "data:empty");
}
