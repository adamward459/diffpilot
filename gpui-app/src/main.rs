mod assets;
mod bench;
mod comparison_control;
mod comparison_menu;
mod format;
mod glyph;
mod pane;
mod repo_view;
mod settings;
mod sidebar;
mod sidebar_rows;
mod theme;
mod workspace;

use bench::Bench;
use diffpilot_core::settings::Settings;
use diffpilot_core::viewed::ViewedStore;
use gpui::{Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use gpui_component::{Root, TitleBar};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use workspace::Workspace;

fn main() {
    let start = Instant::now();
    let bench = bench::Config::from_env();

    Application::new().with_assets(assets::Assets).run(move |cx| {
        gpui_component::init(cx);
        workspace::init(cx);
        if let Some(config) = bench {
            Bench::install(cx, start, config);
        }

        let (settings, viewed_store) = if bench.is_some() {
            let seeded = std::env::var_os("DIFFPILOT_VIEWED_SEED").is_some_and(|path| !path.is_empty());
            let store = if seeded {
                ViewedStore::from_env()
            } else {
                ViewedStore::in_memory()
            };
            (Settings::in_memory(), store)
        } else {
            (Settings::open_default(), ViewedStore::from_env())
        };
        let settings = Arc::new(settings);
        let viewed_store = Arc::new(viewed_store);
        settings::init(settings.clone(), settings::Services::system(), cx);
        let folder = match folder_argument() {
            Some(folder) => {
                settings.remember_folder(&folder);
                Some(folder)
            }
            None => settings.launch_folder(),
        };

        let settings_tab = std::env::var("DIFFPILOT_SETTINGS")
            .ok()
            .as_deref()
            .and_then(settings::Tab::parse);
        if let Some(tab) = settings_tab {
            settings::open(Some(tab), cx);
        } else {
            let bounds = Bounds::centered(None, size(px(1100.), px(700.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitleBar::title_bar_options()),
                window_min_size: Some(size(px(720.), px(420.))),
                ..Default::default()
            };
            cx.open_window(options, move |window, cx| {
                settings::install_theme(window, cx);
                let workspace =
                    cx.new(|cx| Workspace::new(settings, viewed_store, folder, bench.is_some(), window, cx));
                cx.new(|cx| Root::new(workspace, window, cx))
            })
            .expect("open the main window");
        }
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}

fn folder_argument() -> Option<PathBuf> {
    let path = std::env::args_os()
        .skip(1)
        .find(|arg| !arg.to_string_lossy().starts_with('-'))
        .map(PathBuf::from)?;
    let path = path.canonicalize().ok().filter(|path| path.is_dir())?;
    let root = diffpilot_core::git::run(&path, &["rev-parse", "--show-toplevel"]).ok();
    Some(root.map(|root| PathBuf::from(root.trim())).unwrap_or(path))
}
