//! Shows Guide mode over a small fixture repo with a stored guide.
//!
//! `DIFFPILOT_GUIDE_STEP=overview|chapter` picks the screen. `DIFFPILOT_APPEARANCE=light|dark` picks the theme.

use diffpilot::guide::session::Step;
use diffpilot::repo_view::{Nav, RepoView};
use diffpilot::testing::{demo_guide, fixture};
use diffpilot_core::review::ReviewModel;
use diffpilot_core::settings::Settings;
use diffpilot_core::store::{RunId, Store};
use diffpilot_core::viewed::ViewedStore;
use gpui::{AppContext, Application, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let chapter = std::env::var("DIFFPILOT_GUIDE_STEP").is_ok_and(|step| step == "chapter");
    Application::new().with_assets(diffpilot_ui_kit::Assets).run(move |cx| {
        diffpilot_ui_kit::init(cx);
        let repo = fixture();
        let store = Arc::new(Store::open_in_memory().expect("an in-memory store opens"));
        let shared = Rc::new(RefCell::new(None));
        let bounds = Bounds::centered(None, size(px(1388.), px(923.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        let window = {
            let (path, store, shared) = (repo.path().to_path_buf(), store.clone(), shared.clone());
            cx.open_window(options, move |window, cx| {
                diffpilot::settings::install_theme(window, cx);
                let view = cx.new(|cx| {
                    RepoView::new(
                        path,
                        Arc::new(Settings::in_memory()),
                        Arc::new(ViewedStore::in_memory()),
                        store,
                        window,
                        cx,
                    )
                });
                *shared.borrow_mut() = Some(view.clone());
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("open the window")
        };
        let view = shared.borrow().clone().expect("the window builds the view");
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            cx.update(|cx| {
                let review = view.read(cx).review();
                let files: Vec<_> = review
                    .read(cx)
                    .host()
                    .files
                    .iter()
                    .map(|file| (**file).clone())
                    .collect();
                let guide = demo_guide(repo.path(), &files);
                let session = review.read(cx).session_id();
                store.save_draft(session, RunId(1), ReviewModel::STANDARD, &guide).ok();
                store.complete_draft(RunId(1), &guide).ok();
                view.read(cx).guide().update(cx, |guide, cx| {
                    guide.reload(cx);
                    if chapter {
                        guide.go(Step::Chapter(0), cx);
                    }
                });
                window
                    .update(cx, |_, window, cx| {
                        view.update(cx, |view, cx| view.show_nav(Nav::Guide, window, cx))
                    })
                    .ok();
            })
            .ok();
        })
        .detach();
        cx.activate(true);
    });
}
