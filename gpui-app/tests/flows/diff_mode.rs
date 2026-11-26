use diffpilot::repo_view::RepoView;
use diffpilot::settings::{self, Services, Tab};
use diffpilot::testing::{canned_claude, fixture, open_repo, pick_comparison};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::DiffMode;
use diffpilot_core::github::auth::{Credentials, GhCli, MemoryStore};
use diffpilot_core::github::http::{Http, HttpError, Request, Response};
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px, size};
use gpui_component::Root;
use std::sync::Arc;

struct NoGh;

impl GhCli for NoGh {
    fn token(&self, _: &str) -> Option<String> {
        None
    }
}

struct Offline;

impl Http for Offline {
    fn send(&self, _: &Request) -> Result<Response, HttpError> {
        Err(HttpError("offline".into()))
    }
}

fn pane_mode(view: &Entity<RepoView>, cx: &mut VisualTestContext) -> DiffMode {
    let pane = view.read_with(cx, |view, _| view.pane());
    pane.read_with(cx, |pane, _| pane.state().mode())
}

#[gpui::test]
fn the_diff_mode_chosen_in_settings_applies_to_every_session_of_a_repo_opened_after(app: &mut TestAppContext) {
    let settings = Arc::new(Settings::in_memory());
    let services = Services {
        credentials: Credentials::new(Arc::new(MemoryStore::default()), Arc::new(NoGh)),
        http: Arc::new(Offline),
        host: "github.com".into(),
        mcp_server: None,
    };
    app.update(diffpilot_ui_kit::init);
    let window = app.update(|cx| {
        settings::init(settings.clone(), services, cx);
        settings::open(Some(Tab::Diff), cx).expect("the settings window opens")
    });
    let root: Entity<Root> = window.root(app).expect("the settings root");
    let settings_cx = VisualTestContext::from_window(window.into(), app).into_mut();
    let settle = |cx: &mut VisualTestContext| {
        let root = root.clone();
        cx.run_until_parked();
        cx.draw(point(px(0.), px(0.)), size(px(780.), px(800.)), move |_, _| root);
        cx.run_until_parked();
    };
    settle(settings_cx);

    assert_eq!(settings.diff_mode(), DiffMode::Unified);
    let split = settings_cx
        .debug_bounds("layout:Split")
        .expect("the Split choice is on screen");
    settings_cx.simulate_click(split.center(), Modifiers::default());
    settle(settings_cx);
    assert_eq!(settings.diff_mode(), DiffMode::Split, "the choice is saved");

    let dir = fixture();
    let store = Arc::new(ViewedStore::in_memory());
    let (view, cx) = open_repo(app, dir.path(), settings.clone(), store, canned_claude());
    assert_eq!(
        pane_mode(&view, cx),
        DiffMode::Split,
        "a repo opened now starts in Split"
    );

    pick_comparison(&view, Comparison::Staged, cx);
    assert_eq!(pane_mode(&view, cx), DiffMode::Split, "so does the next session");
    pick_comparison(&view, Comparison::WorkingTree, cx);
    assert_eq!(pane_mode(&view, cx), DiffMode::Split);
}
