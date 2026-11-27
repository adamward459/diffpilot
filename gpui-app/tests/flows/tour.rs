use diffpilot::testing::{
    bounds, click, fixture, open_workspace, open_workspace_with_tour, press, shows, wait_for_motion,
};
use diffpilot::workspace::ShowTour;
use diffpilot_core::settings::Settings;
use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, px, size};
use std::sync::Arc;
use std::time::Duration;

fn spotlight(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let mut last = bounds(cx, "tour:spotlight");
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(16));
        let next = bounds(cx, "tour:spotlight");
        if next == last {
            break;
        }
        last = next;
    }
    last
}

fn spotlights(cx: &mut VisualTestContext, selector: &str) -> bool {
    let spot = spotlight(cx);
    spot.contains(&bounds(cx, selector).center())
}

fn open_unseen(app: &mut TestAppContext) -> (&'static mut VisualTestContext, Arc<Settings>, tempfile::TempDir) {
    let dir = fixture();
    let settings = Arc::new(Settings::in_memory());
    let cx = open_workspace_with_tour(app, dir.path(), settings.clone());
    (cx, settings, dir)
}

#[gpui::test]
fn the_first_open_starts_the_tour_and_the_cut_out_wraps_the_real_control(app: &mut TestAppContext) {
    let (cx, settings, _dir) = open_unseen(app);
    assert!(shows(cx, "tour:title:Your repository"));
    assert!(shows(cx, "tour:counter:1 of 8"));
    assert!(!settings.tour_seen(), "the tour counts as seen once it is closed");

    click(cx, "tour:next");
    assert!(shows(cx, "tour:title:What you compare"));
    let spot = spotlight(cx);
    let controls = bounds(cx, "comparison-label");
    assert!(spot.contains(&controls.origin) && spot.contains(&controls.bottom_right()));
    assert!(
        spot.left() > controls.left() - px(16.)
            && spot.right() < controls.right() + px(16.)
            && spot.top() > controls.top() - px(16.)
            && spot.bottom() < controls.bottom() + px(16.),
        "the cut-out hugs the control: {spot:?} around {controls:?}"
    );
}

#[gpui::test]
fn the_cut_out_follows_its_control_when_the_window_resizes(app: &mut TestAppContext) {
    let (cx, _settings, _dir) = open_unseen(app);
    click(cx, "tour:next");
    click(cx, "tour:next");
    assert!(shows(cx, "tour:title:Changed files"));
    let before = spotlight(cx);

    cx.simulate_resize(size(px(1000.), px(500.)));
    let after = spotlight(cx);
    assert!(after.size.height < before.size.height, "{before:?} then {after:?}");
    assert!(
        after.bottom() < px(500.) && after.contains(&bounds(cx, "sidebar:toggle").center()),
        "{after:?}"
    );
}

#[gpui::test]
fn next_and_back_move_between_stops_and_the_last_next_closes(app: &mut TestAppContext) {
    let (cx, settings, _dir) = open_unseen(app);
    assert!(!spotlights(cx, "comparison-label"), "the first stop is the folder");

    click(cx, "tour:next");
    assert!(spotlights(cx, "comparison-label"));
    press(cx, "right");
    assert!(spotlights(cx, "sidebar:toggle"), "the arrow key goes forward");
    press(cx, "left");
    assert!(spotlights(cx, "comparison-label"), "the arrow key goes back");
    click(cx, "tour:back");
    assert!(!spotlights(cx, "comparison-label"), "the Back button goes back");

    for _ in 0..4 {
        press(cx, "enter");
    }
    assert!(shows(cx, "tour:title:Review with Claude"));
    assert!(spotlights(cx, "review:open"), "the Claude pill holds Ask Claude");
    assert!(spotlights(cx, "toolbar:comments"), "and the comments button");
    press(cx, "enter");
    assert!(shows(cx, "tour:title:Push and pull"));
    assert!(spotlights(cx, "remote:status"));
    press(cx, "enter");
    assert!(shows(cx, "tour:title:Commit"));
    assert!(spotlights(cx, "bottom-bar:commit"));
    press(cx, "enter");
    assert!(shows(cx, "tour:counter:8 of 8"));
    assert!(spotlights(cx, "nav-switch"));
    assert!(!settings.tour_seen());

    press(cx, "enter");
    assert!(settings.tour_seen(), "the last Next closes the tour");
    click(cx, "comparison-label");
    assert!(
        shows(cx, "open-folder:dialog"),
        "the app answers again once the tour is gone"
    );
}

#[gpui::test]
fn escape_closes_the_tour_and_a_later_open_does_not_start_it(app: &mut TestAppContext) {
    let (cx, settings, dir) = open_unseen(app);
    press(cx, "escape");
    assert!(settings.tour_seen());
    click(cx, "comparison-label");
    assert!(shows(cx, "open-folder:dialog"));

    let again = open_workspace_with_tour(app, dir.path(), settings);
    assert!(
        !shows(again, "tour:card"),
        "a user who saw the tour is not shown it again"
    );
}

#[gpui::test]
fn help_show_tour_starts_it_again(app: &mut TestAppContext) {
    let dir = fixture();
    let settings = Arc::new(Settings::in_memory());
    let cx = open_workspace(app, dir.path(), settings);
    assert!(!shows(cx, "tour:card"), "a tour that was seen does not start by itself");

    cx.dispatch_action(ShowTour);
    assert!(shows(cx, "tour:title:Your repository"));
    assert!(shows(cx, "tour:counter:1 of 8"));
}

#[gpui::test]
fn a_stop_whose_element_is_hidden_is_skipped_and_not_counted(app: &mut TestAppContext) {
    let dir = fixture();
    let settings = Arc::new(Settings::in_memory());
    let cx = open_workspace(app, dir.path(), settings);
    press(cx, "ctrl-cmd-s");
    wait_for_motion(cx);
    cx.dispatch_action(ShowTour);
    assert!(
        shows(cx, "tour:counter:1 of 6"),
        "the sidebar and the Guide switch are hidden"
    );

    press(cx, "right");
    press(cx, "right");
    assert!(shows(cx, "tour:counter:3 of 6"));
    assert!(
        spotlights(cx, "pane:copy-path:docs/committed.md"),
        "the file list is skipped"
    );
    press(cx, "left");
    assert!(spotlights(cx, "comparison-label"), "going back skips it too");
}

#[gpui::test]
fn the_scrim_blocks_the_app_and_app_shortcuts_while_the_tour_is_open(app: &mut TestAppContext) {
    let (cx, settings, _dir) = open_unseen(app);
    press(cx, "ctrl-cmd-s");
    click(cx, "tour:next");
    click(cx, "tour:next");
    assert!(
        shows(cx, "tour:title:Changed files"),
        "the shortcut did not hide the sidebar"
    );

    click(cx, "comparison-label");
    assert!(settings.tour_seen(), "a click on the scrim closes the tour");
    assert!(
        !shows(cx, "open-folder:dialog"),
        "the click did not reach the control under the scrim"
    );
    click(cx, "comparison-label");
    assert!(shows(cx, "open-folder:dialog"));
}
