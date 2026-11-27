//! Opens the app window on `DIFFPILOT_REPO` with a fake `claude`, brings it to the state `DIFFPILOT_SCENE` names,
//! and prints `shot <x> <y> <width> <height> <window width> <window height>` for the part to capture.

use diffpilot::guide::session::Step;
use diffpilot::repo_view::{Nav, RepoView};
use diffpilot::runs::Runs;
use diffpilot::testing::{FakeClaude, fake_engine, guide_over, offline_services};
use diffpilot::workspace::{self, Workspace};
use diffpilot_core::changes::FileStatus;
use diffpilot_core::diff::{FileDiff, LineKind};
use diffpilot_core::review::{Finding, ReviewModel, ReviewResult, Severity};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::RunId;
use diffpilot_core::threads::{Side, ThreadAnchor};
use diffpilot_core::viewed::ViewedStore;
use diffpilot_ui_kit::regions::region_bounds;
use gpui::{
    AnyWindowHandle, App, AppContext, Application, AsyncApp, Bounds, Context, Entity, Pixels, TitlebarOptions, Window,
    WindowBounds, WindowOptions, point, px, size,
};
use gpui_component::{Root, TitleBar};
use std::cell::RefCell;
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const STEP: Duration = Duration::from_millis(100);
const LIMIT: Duration = Duration::from_secs(40);
const LONG_LINE_FILE: &str = "src/components/inbox/MessageRow.tsx";

struct Stage {
    cx: AsyncApp,
    window: AnyWindowHandle,
    workspace: Entity<Workspace>,
    claude: FakeClaude,
}

impl Stage {
    async fn pause(&self, time: Duration) {
        self.cx.background_executor().timer(time).await;
    }

    async fn until<T>(&self, what: &str, mut check: impl FnMut(&App) -> Option<T>) -> T {
        let started = Instant::now();
        loop {
            if let Ok(Some(value)) = self.cx.update(|cx| check(cx)) {
                return value;
            }
            assert!(started.elapsed() < LIMIT, "timed out waiting for {what}");
            self.pause(STEP).await;
        }
    }

    /// The repo view once its diff is loaded.
    async fn view(&self) -> Entity<RepoView> {
        let workspace = self.workspace.clone();
        let view = self
            .until("the repo view", |cx| workspace.read(cx).active_view().cloned())
            .await;
        let loaded = view.clone();
        self.until("the diff", move |cx| {
            let view = loaded.read(cx);
            view.pill()?;
            (!view.review().read(cx).host().files.is_empty()).then_some(())
        })
        .await;
        view
    }

    /// The region's bounds once they have stopped moving.
    async fn settled(&self, region: &str) -> Bounds<Pixels> {
        let mut last = None;
        let mut steady = 0;
        let started = Instant::now();
        while steady < 5 {
            self.pause(STEP).await;
            let now = region_bounds(region);
            steady = if now.is_some() && now == last { steady + 1 } else { 0 };
            last = now;
            assert!(started.elapsed() < LIMIT, "the region `{region}` did not show");
        }
        last.expect("a steady region")
    }

    async fn with_view<R>(
        &self,
        view: &Entity<RepoView>,
        change: impl FnOnce(&mut RepoView, &mut Window, &mut Context<RepoView>) -> R,
    ) -> R {
        self.cx
            .clone()
            .update_window(self.window, |_, window, cx| {
                view.update(cx, |view, cx| change(view, window, cx))
            })
            .expect("the window is open")
    }

    fn files(&self, view: &Entity<RepoView>) -> Vec<Arc<FileDiff>> {
        self.cx
            .update(|cx| view.read(cx).review().read(cx).host().files.to_vec())
            .expect("the app is running")
    }

    /// Waits until the pane has highlighted its files and any preview has loaded.
    async fn quiet(&self, view: &Entity<RepoView>) {
        self.pause(Duration::from_millis(800)).await;
        let pane = self.cx.update(|cx| view.read(cx).pane()).expect("the app is running");
        self.until("the highlighting", |cx| {
            (!pane.read(cx).is_highlighting()).then_some(())
        })
        .await;
        self.pause(Duration::from_millis(800)).await;
    }

    async fn play(&self, scene: &str) -> &'static str {
        let view = self.view().await;
        let region = match scene {
            "review-setup" => self.review_setup(&view).await,
            "review-threads" => self.review_threads(&view).await,
            "review-fix-picker" => self.review_fix_picker(&view).await,
            "review-claude-area" => self.review_claude_area(&view).await,
            "pane" => "pane",
            "pane-wrap" => self.pane_long_lines(&view, true).await,
            "pane-nowrap" => self.pane_long_lines(&view, false).await,
            "pane-preview" => self.pane_preview(&view).await,
            "guide-overview" => self.guide(&view, false).await,
            "guide-chapter" => self.guide(&view, true).await,
            "sidebar" => "sidebar",
            "toolbar" => "toolbar",
            "settings" => self.settings().await,
            other => panic!("no scene named `{other}`"),
        };
        self.quiet(&view).await;
        region
    }

    async fn review_setup(&self, view: &Entity<RepoView>) -> &'static str {
        let pill = self
            .cx
            .update(|cx| view.read(cx).pill())
            .ok()
            .flatten()
            .expect("the view has a session");
        pill.update(&mut self.cx.clone(), |pill, cx| pill.set_open(true, cx))
            .expect("the pill is alive");
        "popover:review-popover"
    }

    /// Starts a review that ends with findings on the first files that have added lines.
    async fn start_review(&self, view: &Entity<RepoView>) {
        let findings = self
            .files(view)
            .iter()
            .filter(|file| reviewable(file))
            .filter_map(|file| Some((file.path.clone(), added_line(file)?)))
            .take(2)
            .flat_map(|(path, line)| {
                let finding = |line, severity, title: &str, body: &str| Finding {
                    path: path.clone(),
                    line,
                    severity,
                    title: title.into(),
                    body: body.into(),
                };
                [
                    finding(
                        Some(line),
                        Severity::MustFix,
                        "Empty case",
                        "Guard this against an empty value.",
                    ),
                    finding(
                        None,
                        Severity::Nit,
                        "Helper name",
                        "Pick a name that says what it returns.",
                    ),
                ]
            })
            .collect();
        self.claude.review_with(&ReviewResult {
            summary: "The change is sound. Two spots need a closer look.".into(),
            findings,
        });
        let review = self.cx.update(|cx| view.read(cx).review()).expect("the app is running");
        review
            .update(&mut self.cx.clone(), |review, cx| review.start_review(cx))
            .expect("the review is alive");
    }

    async fn finish_review(&self, view: &Entity<RepoView>) {
        self.claude.finish();
        let review = self.cx.update(|cx| view.read(cx).review()).expect("the app is running");
        self.until("the review", |cx| {
            let review = review.read(cx);
            (!review.reviewing(cx) && review.thread_count() > 0).then_some(())
        })
        .await;
    }

    async fn review_threads(&self, view: &Entity<RepoView>) -> &'static str {
        self.start_review(view).await;
        self.finish_review(view).await;
        let files = self.files(view);
        let path = files
            .iter()
            .find(|file| reviewable(file))
            .expect("a reviewable file")
            .path
            .clone();
        let pane = self.cx.update(|cx| view.read(cx).pane()).expect("the app is running");
        pane.update(&mut self.cx.clone(), |pane, cx| pane.reveal(&path, cx))
            .expect("the pane is alive");
        "pane"
    }

    async fn review_fix_picker(&self, view: &Entity<RepoView>) -> &'static str {
        self.start_review(view).await;
        self.finish_review(view).await;
        let files = self.files(view);
        let file = files.iter().find(|file| reviewable(file)).expect("a reviewable file");
        let line = added_line(file).expect("an added line");
        let review = self.cx.update(|cx| view.read(cx).review()).expect("the app is running");
        review
            .update(&mut self.cx.clone(), |review, cx| {
                let id = review.create_thread(ThreadAnchor::line(&file.path, Side::New, line), cx);
                review.post(id, "Can this be split into two functions?", cx);
            })
            .expect("the review is alive");
        let fix = self
            .cx
            .update(|cx| view.read(cx).fix_button())
            .ok()
            .flatten()
            .expect("the view has a session");
        self.until("the Fix button", |cx| {
            (!review.read(cx).fixable(cx).is_empty()).then_some(())
        })
        .await;
        fix.update(&mut self.cx.clone(), |fix, cx| fix.set_open(true, cx))
            .expect("the button is alive");
        "popover:fix-popover"
    }

    async fn review_claude_area(&self, view: &Entity<RepoView>) -> &'static str {
        self.start_review(view).await;
        self.until("the run to start", |_| (self.claude.launches() > 0).then_some(()))
            .await;
        self.pause(Duration::from_secs(2)).await;
        self.with_view(view, |view, _, cx| view.toggle_claude_area(cx)).await;
        "claude-area"
    }

    async fn pane_long_lines(&self, view: &Entity<RepoView>, wrap: bool) -> &'static str {
        let pane = self.cx.update(|cx| view.read(cx).pane()).expect("the app is running");
        pane.update(&mut self.cx.clone(), |pane, cx| {
            pane.set_wrap(wrap, cx);
            pane.reveal(LONG_LINE_FILE, cx);
        })
        .expect("the pane is alive");
        "pane"
    }

    async fn pane_preview(&self, view: &Entity<RepoView>) -> &'static str {
        let files = self.files(view);
        let path = files
            .iter()
            .find(|file| file.path.ends_with(".md"))
            .expect("a Markdown file in the diff")
            .path
            .clone();
        let pane = self.cx.update(|cx| view.read(cx).pane()).expect("the app is running");
        pane.update(&mut self.cx.clone(), |pane, cx| {
            pane.reveal(&path, cx);
            pane.set_preview(&path, true, cx);
        })
        .expect("the pane is alive");
        "pane"
    }

    async fn guide(&self, view: &Entity<RepoView>, chapter: bool) -> &'static str {
        let files: Vec<FileDiff> = self.files(view).iter().map(|file| (**file).clone()).collect();
        let (review, guide) = self
            .cx
            .update(|cx| (view.read(cx).review(), view.read(cx).guide()))
            .expect("the app is running");
        let session = self
            .cx
            .update(|cx| review.read(cx).session_id())
            .expect("the app is running");
        let repo = PathBuf::from(std::env::var_os("DIFFPILOT_REPO").expect("DIFFPILOT_REPO names the repo"));
        let stored = guide_over(&repo, &files);
        let store = self.claude.store();
        store
            .save_draft(session, RunId(1), ReviewModel::STANDARD, &stored)
            .expect("the draft saves");
        store.complete_draft(RunId(1), &stored).expect("the draft completes");
        guide
            .update(&mut self.cx.clone(), |guide, cx| guide.reload(cx))
            .expect("the guide is alive");
        self.until("the guide", |cx| {
            (!guide.read(cx).reading().chapters.is_empty()).then_some(())
        })
        .await;
        if chapter {
            guide
                .update(&mut self.cx.clone(), |guide, cx| guide.go(Step::Chapter(0), cx))
                .expect("the guide is alive");
        }
        self.with_view(view, |view, window, cx| view.show_nav(Nav::Guide, window, cx))
            .await;
        "content"
    }

    async fn settings(&self) -> &'static str {
        let workspace = self.workspace.clone();
        self.cx
            .clone()
            .update_window(self.window, |_, window, cx| {
                workspace.update(cx, |workspace, cx| workspace.open_settings(None, window, cx))
            })
            .expect("the window is open");
        "dialog:settings"
    }
}

fn reviewable(file: &FileDiff) -> bool {
    file.status == FileStatus::Modified && !file.path.ends_with(".md") && added_line(file).is_some()
}

fn added_line(file: &FileDiff) -> Option<u32> {
    file.hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .find(|line| line.kind == LineKind::Added)
        .and_then(|line| line.new)
}

fn main() {
    let scene = std::env::var("DIFFPILOT_SCENE").expect("DIFFPILOT_SCENE names the scene");
    let folder = PathBuf::from(std::env::var_os("DIFFPILOT_REPO").expect("DIFFPILOT_REPO names the repo"));
    Application::new().with_assets(diffpilot_ui_kit::Assets).run(move |cx| {
        diffpilot_ui_kit::init(cx);
        workspace::init(cx);
        let claude = fake_engine();
        let settings = Arc::new(Settings::in_memory());
        settings.set_tour_seen(true);
        let store = claude.store();
        let viewed = Arc::new(ViewedStore::from_env(store.clone()));
        diffpilot::settings::init(settings.clone(), offline_services(store.clone()), cx);
        cx.set_global(Runs::with_engine(claude.engine()));

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1388.), px(923.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                traffic_light_position: Some(point(px(20.), px(19.))),
                ..TitleBar::title_bar_options()
            }),
            ..Default::default()
        };
        let shared = Rc::new(RefCell::new(None));
        let window = {
            let shared = shared.clone();
            cx.open_window(options, move |window, cx| {
                diffpilot::settings::install_theme(window, cx);
                let workspace = cx.new(|cx| Workspace::new(settings, viewed, store, Some(folder), false, window, cx));
                *shared.borrow_mut() = Some(workspace.clone());
                cx.new(|cx| Root::new(workspace, window, cx))
            })
            .expect("open the main window")
        };
        let workspace = shared.borrow().clone().expect("the window builds the workspace");
        cx.spawn(async move |cx| {
            let stage = Stage {
                cx: cx.clone(),
                window: window.into(),
                workspace,
                claude,
            };
            let region = stage.play(&scene).await;
            let part = stage.settled(region).await;
            let whole = stage
                .cx
                .clone()
                .update_window(stage.window, |_, window, _| window.viewport_size())
                .expect("the window is open");
            println!(
                "shot {} {} {} {} {} {}",
                f32::from(part.origin.x),
                f32::from(part.origin.y),
                f32::from(part.size.width),
                f32::from(part.size.height),
                f32::from(whole.width),
                f32::from(whole.height),
            );
            std::io::stdout().flush().expect("stdout is open");
            std::future::pending::<()>().await;
        })
        .detach();
        cx.activate(true);
    });
}
