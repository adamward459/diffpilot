use diffpilot::testing::{
    FakeClaude, canned_review, click, fake_claude, fixture_named, open_workspace, press, shows, wait_for_motion,
    wait_until, wait_until_shows,
};
use diffpilot_core::settings::Settings;
use gpui::{TestAppContext, VisualTestContext};
use std::path::PathBuf;
use std::sync::Arc;

struct Folders {
    fake: FakeClaude,
    alpha: PathBuf,
    beta: PathBuf,
    _dirs: Vec<tempfile::TempDir>,
}

impl Folders {
    fn idle(&self, folder: &std::path::Path) -> bool {
        self.fake.engine().active(folder).is_none()
    }
}

fn open_with_running_review(
    app: &mut TestAppContext,
    settings: Arc<Settings>,
) -> (Folders, &'static mut VisualTestContext) {
    let fake = fake_claude(app);
    fake.review_with(&canned_review());
    fake.hold();
    let mut dirs = Vec::new();
    let mut folder = |name: &str| {
        let (dir, path) = fixture_named(name);
        dirs.push(dir);
        let path = path.canonicalize().expect("folder");
        settings.remember_folder(&path);
        path
    };
    let (gamma, beta, alpha) = (folder("gamma"), folder("beta"), folder("alpha"));
    let _ = gamma;
    let cx = open_workspace(app, &alpha, settings);
    start_review(cx, &fake, 1);
    (
        Folders {
            fake,
            alpha,
            beta,
            _dirs: dirs,
        },
        cx,
    )
}

fn start_review(cx: &mut VisualTestContext, fake: &FakeClaude, launches: usize) {
    click(cx, "review:open");
    click(cx, "review:start");
    fake.wait_for_launches(cx, launches);
}

fn pick_folder(cx: &mut VisualTestContext, name: &str) {
    click(cx, "folder-title");
    click(cx, &format!("folder:{name}"));
}

#[gpui::test]
fn cancel_keeps_the_folder_and_the_run(app: &mut TestAppContext) {
    let settings = Arc::new(Settings::in_memory());
    let (folders, cx) = open_with_running_review(app, settings.clone());

    pick_folder(cx, "beta");
    assert!(shows(cx, "switch-alert:dialog"), "a running review asks first");
    assert!(shows(cx, "switch-alert:title"));
    assert!(!shows(cx, "folder-active:beta"), "nothing switched yet");

    press(cx, "escape");
    wait_for_motion(cx);
    assert!(!shows(cx, "folder-active:beta"), "Escape keeps the folder");
    assert!(!folders.idle(&folders.alpha), "and the run keeps going");
    assert!(settings.ask_before_stopping_runs());

    press(cx, "ctrl-cmd-2");
    click(cx, "switch-alert:cancel");
    wait_for_motion(cx);
    assert!(
        !shows(cx, "folder-active:beta"),
        "the shortcut asks too, and Cancel keeps the folder"
    );
    assert!(!folders.idle(&folders.alpha));
}

#[gpui::test]
fn stop_and_switch_stops_the_run_and_opens_the_folder(app: &mut TestAppContext) {
    let settings = Arc::new(Settings::in_memory());
    let (folders, cx) = open_with_running_review(app, settings.clone());

    pick_folder(cx, "beta");
    press(cx, "enter");
    wait_for_motion(cx);
    assert!(shows(cx, "folder-active:beta"), "Enter stops the run and switches");
    assert!(folders.idle(&folders.alpha), "the run in alpha is stopped");
    assert!(
        !shows(cx, "switch-toast"),
        "the alert was the warning, so there is no toast"
    );
    assert!(settings.ask_before_stopping_runs(), "it asks again next time");

    pick_folder(cx, "alpha");
    wait_until_shows(cx, "claude:status:Review stopped");
}

#[gpui::test]
fn dont_ask_again_stops_without_asking_and_offers_to_go_back(app: &mut TestAppContext) {
    let settings = Arc::new(Settings::in_memory());
    let (folders, cx) = open_with_running_review(app, settings.clone());

    pick_folder(cx, "beta");
    click(cx, "switch-alert:ask-again");
    click(cx, "switch-alert:confirm");
    wait_for_motion(cx);
    assert!(shows(cx, "folder-active:beta"));
    assert!(!settings.ask_before_stopping_runs(), "Don't ask again is saved");

    start_review(cx, &folders.fake, 2);
    assert!(!folders.idle(&folders.beta));
    pick_folder(cx, "gamma");
    assert!(shows(cx, "folder-active:gamma"), "the second switch has no alert");
    assert!(shows(cx, "switch-toast"), "a toast says what stopped");
    assert!(folders.idle(&folders.beta));

    click(cx, "switch-toast:back");
    wait_until_shows(cx, "claude:status:Review stopped");
}

fn pick_staged_base_for_beta(cx: &mut VisualTestContext) {
    press(cx, "cmd-o");
    click(cx, "open:folder:beta");
    wait_until_shows(cx, "open-base");
    click(cx, "open-base");
    click(cx, "open-base:Staged Changes Only");
    click(cx, "open-folder-confirm");
    assert!(shows(cx, "switch-alert:dialog"), "the sheet asks like the menu does");
}

fn saved_for(settings: &Settings, folder: &std::path::Path) -> Option<diffpilot_core::comparison::Comparison> {
    settings.comparison(&diffpilot_core::comparison::store_key(folder, Some("feature")))
}

#[gpui::test]
fn the_sheet_comparison_applies_after_stop_and_switch(app: &mut TestAppContext) {
    let settings = Arc::new(Settings::in_memory());
    let (folders, cx) = open_with_running_review(app, settings.clone());

    pick_staged_base_for_beta(cx);
    press(cx, "enter");
    wait_for_motion(cx);
    assert!(shows(cx, "folder-active:beta"));
    wait_until(cx, "the staged comparison is saved", || {
        saved_for(&settings, &folders.beta) == Some(diffpilot_core::comparison::Comparison::Staged)
    });
}

#[gpui::test]
fn cancelling_the_alert_drops_the_sheet_comparison(app: &mut TestAppContext) {
    let settings = Arc::new(Settings::in_memory());
    let (folders, cx) = open_with_running_review(app, settings.clone());

    pick_staged_base_for_beta(cx);
    click(cx, "switch-alert:cancel");
    wait_for_motion(cx);
    assert!(!shows(cx, "folder-active:beta"));
    press(cx, "escape");
    wait_for_motion(cx);

    pick_folder(cx, "beta");
    press(cx, "enter");
    wait_for_motion(cx);
    assert!(shows(cx, "folder-active:beta"));
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        diffpilot::testing::draw(cx);
    }
    assert_eq!(saved_for(&settings, &folders.beta), None, "beta opened as it was");
}
