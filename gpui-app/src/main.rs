use diffpilot::bench::{self, Bench};
use diffpilot::runs::Runs;
use diffpilot::settings;
use diffpilot::workspace::{self, Workspace};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::Store;
use diffpilot_core::viewed::ViewedStore;
use gpui::{
    Application, Bounds, TitlebarOptions, WindowBounds, WindowKind, WindowOptions, point, prelude::*, px, size,
};
use gpui_component::{Root, TitleBar};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[global_allocator]
static ALLOCATOR: bench::Counting = bench::Counting;

fn main() {
    let start = Instant::now();
    let bench = bench::Config::from_env();
    if bench.is_some() {
        bench::Counting::enable();
    }

    Application::new().with_assets(diffpilot_ui_kit::Assets).run(move |cx| {
        diffpilot_ui_kit::init(cx);
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
        let review_store = Arc::new(open_review_store(bench.is_some()));
        let viewed_store = Arc::new(viewed_store);
        settings::init(settings.clone(), settings::Services::system(), cx);
        if bench.is_none() {
            cx.set_global(Runs::start(review_store.clone(), &settings));
            let settings = settings.clone();
            cx.observe_global::<settings::SettingsChanged>(move |cx| {
                if let Some(runs) = cx.try_global::<Runs>() {
                    runs.use_settings(&settings);
                }
            })
            .detach();
        }
        let folder = match folder_argument() {
            Some(folder) => Some(folder),
            None => settings.launch_folder(),
        };

        let settings_tab = std::env::var("DIFFPILOT_SETTINGS")
            .ok()
            .as_deref()
            .and_then(settings::Tab::parse);
        if let Some(tab) = settings_tab {
            settings::open(Some(tab), cx);
        } else {
            let bounds = Bounds::centered(None, size(px(1388.), px(923.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                kind: if bench.is_some() {
                    WindowKind::PopUp
                } else {
                    WindowKind::Normal
                },
                titlebar: Some(TitlebarOptions {
                    traffic_light_position: Some(point(px(20.), px(19.))),
                    ..TitleBar::title_bar_options()
                }),
                window_min_size: Some(size(px(720.), px(420.))),
                ..Default::default()
            };
            cx.open_window(options, move |window, cx| {
                settings::install_theme(window, cx);
                let workspace = cx.new(|cx| {
                    Workspace::new(
                        settings,
                        viewed_store,
                        review_store,
                        folder,
                        bench.is_some(),
                        window,
                        cx,
                    )
                });
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

fn open_review_store(in_memory: bool) -> Store {
    let memory = || Store::open_in_memory().expect("an in-memory sqlite database opens");
    if in_memory {
        return memory();
    }
    Store::open_default().unwrap_or_else(|error| {
        eprintln!("diffpilot: {error}. Review threads will not be saved.");
        memory()
    })
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
