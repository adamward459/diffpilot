use diffpilot::testing::{click, open_preview, shows, wait_until_shows};
use gpui::TestAppContext;

#[gpui::test]
fn images_show_a_preview_until_the_diff_is_chosen(app: &mut TestAppContext) {
    let (_opened, cx) = open_preview(app);
    wait_until_shows(cx, "pane:preview-card:assets/logo.png");
    click(cx, "pane:preview-switch:assets/logo.png:Diff");
    assert!(shows(cx, "pane:notice:assets/logo.png"));
}

#[gpui::test]
fn markdown_shows_the_diff_until_preview_is_chosen(app: &mut TestAppContext) {
    let (_opened, cx) = open_preview(app);
    wait_until_shows(cx, "pane:hunk:notes.md:0");
    assert!(!shows(cx, "pane:preview-card:notes.md"));
    click(cx, "pane:preview-switch:notes.md:Preview");
    wait_until_shows(cx, "pane:preview-card:notes.md");
}

#[gpui::test]
fn a_changed_image_switches_between_two_up_swipe_and_onion_skin(app: &mut TestAppContext) {
    let (_opened, cx) = open_preview(app);
    wait_until_shows(cx, "pane:image-mode:assets/logo.png:Swipe");
    click(cx, "pane:image-mode:assets/logo.png:Swipe");
    assert!(shows(cx, "pane:image-stage:assets/logo.png"));
    click(cx, "pane:image-stage:assets/logo.png");

    click(cx, "pane:image-mode:assets/logo.png:Onion Skin");
    assert!(shows(cx, "pane:onion-track:assets/logo.png"));
    click(cx, "pane:onion-track:assets/logo.png");
}

#[gpui::test]
fn an_added_image_has_no_compare_modes(app: &mut TestAppContext) {
    let (_opened, cx) = open_preview(app);
    wait_until_shows(cx, "pane:image-mode:assets/logo.png:2-up");
    assert!(shows(cx, "pane:preview-card:assets/icon.png"));
    assert!(!shows(cx, "pane:image-mode:assets/icon.png:2-up"));
}
