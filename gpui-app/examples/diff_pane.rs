use diffpilot::bench::{Counting, Meter, live_bytes};
use diffpilot::diff_pane::highlight;
use diffpilot::diff_pane::state::{Geometry, PaneState};
use diffpilot::diff_pane::{DiffPane, DiffPaneEvent};
use diffpilot::review::{ReviewHost, ReviewSession};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::{self, DiffMode, FileDiff, LineKind};
use diffpilot_core::git;
use diffpilot_core::settings::{self, Settings};
use diffpilot_core::store::Store;
use diffpilot_core::threads::{Side, ThreadAnchor};
use diffpilot_ui_kit::Assets;
use gpui::{
    AnyWindowHandle, App, AppContext as _, Application, AsyncApp, Bounds, Entity, Window, WindowBounds, WindowKind,
    WindowOptions, point, px, size,
};
use gpui_component::Root;
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const BUDGET_MS: f32 = 1000.0 / 120.0;
const WINDOW: (f32, f32) = (1200.0, 800.0);
const REOPEN_CYCLES: usize = 5;
const SWEEP_FRAMES: usize = 240;
const FLICK_FRAMES: usize = 120;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn ms(from: Instant) -> f64 {
    from.elapsed().as_secs_f64() * 1000.0
}

fn repo_path() -> PathBuf {
    std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("DIFFPILOT_REPO").map(PathBuf::from))
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.fixtures/large"))
}

fn preview_enabled() -> bool {
    std::env::var("DIFFPILOT_PREVIEW").is_ok_and(|value| value == "1")
}

fn gradient(width: u32, height: u32, from: [u8; 3], to: [u8; 3]) -> image::RgbaImage {
    image::RgbaImage::from_fn(width, height, |x, y| {
        let t = (x + y) as f32 / (width + height) as f32;
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
        image::Rgba([mix(from[0], to[0]), mix(from[1], to[1]), mix(from[2], to[2]), 255])
    })
}

fn preview_repo() -> PathBuf {
    let repo = std::env::temp_dir().join(format!("diffpilot-preview-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(repo.join("assets")).expect("mkdir");
    std::fs::create_dir_all(repo.join("docs")).expect("mkdir");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    gradient(64, 48, [250, 160, 80], [240, 80, 120])
        .save(repo.join("assets/logo.png"))
        .expect("png");
    std::fs::write(
        repo.join("docs/guide.md"),
        "# Guide\n\nRun the **app** with `cargo run`.\n\n- build it\n- open a repo\n",
    )
    .expect("md");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "base"]);
    git(&["checkout", "-q", "-b", "feature"]);
    gradient(96, 64, [60, 120, 250], [120, 220, 180])
        .save(repo.join("assets/logo.png"))
        .expect("png");
    gradient(32, 32, [90, 90, 90], [220, 220, 220])
        .save(repo.join("assets/icon.png"))
        .expect("png");
    std::fs::write(
        repo.join("docs/guide.md"),
        "# Guide\n\nRun the **app** with `cargo run`, then open a repo.\n\n1. build it\n2. open a repo\n3. review the diff\n\n> Previews show up for images and Markdown.\n\n| Key | Action |\n| --- | --- |\n| `j` | next file |\n| `k` | previous file |\n\n```rust\nfn main() {}\n```\n",
    )
    .expect("md");
    std::fs::write(
        repo.join("docs/new.md"),
        "## Release notes\n\nA *new* page with a [link](https://example.com) and ~~old~~ text.\n",
    )
    .expect("md");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "feature"]);
    repo
}

fn viewed_seed(repo: &Path) -> HashSet<String> {
    let path = std::env::var_os("DIFFPILOT_VIEWED_SEED")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.with_file_name("large-viewed.json"));
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .map(|paths| paths.into_iter().collect())
        .unwrap_or_default()
}

fn load(repo: &Path) -> (Vec<FileDiff>, Value) {
    let args: Vec<String> = diff::DIFF_ARGS
        .iter()
        .map(|arg| arg.to_string())
        .chain(Comparison::checked_out("main", true).revisions())
        .collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let started = Instant::now();
    let text = git::run(repo, &args).expect("git diff failed");
    let git_ms = ms(started);
    let started = Instant::now();
    let parsed = diff::parse(&text);
    let parse_ms = ms(started);
    let started = Instant::now();
    let files = diff::in_tree_order(parsed);
    let order_ms = ms(started);
    (
        files,
        json!({ "git_diff_ms": git_ms, "parse_ms": parse_ms, "tree_order_ms": order_ms }),
    )
}

fn highlight_all(files: &[FileDiff], cold: bool) -> Value {
    let started = Instant::now();
    highlight::engine();
    let engine_ms = ms(started);
    let started = Instant::now();
    for file in files {
        std::hint::black_box(highlight::engine().highlight(file));
    }
    let one_thread_ms = ms(started);
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let next = AtomicUsize::new(0);
    let started = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                while let Some(file) = files.get(next.fetch_add(1, Ordering::Relaxed)) {
                    std::hint::black_box(highlight::engine().highlight(file));
                }
            });
        }
    });
    let mut report = json!({ "one_thread_ms": one_thread_ms, "all_cores_ms": ms(started) });
    if cold {
        report["engine_load_ms"] = json!(engine_ms);
    }
    report
}

fn headless(repo: &Path, cold: bool) -> Value {
    let (files, mut report) = load(repo);
    report["highlight"] = highlight_all(&files, cold);
    let mut state = PaneState::new(Geometry {
        width: WINDOW.0,
        height: WINDOW.1,
        cell: 7.2,
    });
    let started = Instant::now();
    state.replace_files(files);
    report["rows_ms"] = json!(ms(started));
    let started = Instant::now();
    state.toggle_collapsed("src/api/client.ts");
    report["toggle_collapse_ms"] = json!(ms(started));
    let started = Instant::now();
    state.set_mode(DiffMode::Split);
    report["toggle_split_ms"] = json!(ms(started));
    let started = Instant::now();
    state.set_mode(DiffMode::Unified);
    report["toggle_unified_ms"] = json!(ms(started));
    report
}

fn flatten(prefix: &str, value: &Value, into: &mut Map<String, Value>) {
    match value {
        Value::Object(object) => {
            for (key, inner) in object {
                flatten(&format!("{prefix}_{key}"), inner, into);
            }
        }
        number => {
            into.insert(prefix.trim_start_matches('_').to_string(), number.clone());
        }
    }
}

fn stats(prefix: &str, samples: &[f32], into: &mut Map<String, Value>) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f32::total_cmp);
    let Some(&max) = sorted.last() else { return };
    let at = |q: f32| sorted[((sorted.len() as f32 * q) as usize).min(sorted.len() - 1)];
    into.insert(format!("{prefix}_median_ms"), json!(at(0.5)));
    into.insert(format!("{prefix}_p95_ms"), json!(at(0.95)));
    into.insert(format!("{prefix}_max_ms"), json!(max));
    into.insert(
        format!("{prefix}_over_budget"),
        json!(sorted.iter().filter(|&&sample| sample > BUDGET_MS).count()),
    );
}

struct Phase {
    name: &'static str,
    mode: DiffMode,
    wrap: bool,
    frames: usize,
    step: Step,
}

enum Step {
    Sweep,
    Pixels(f32),
}

struct Run {
    phase: Phase,
    step_px: f32,
    done: usize,
    last_tick: Option<Instant>,
    intervals: Vec<f32>,
    meter: Meter,
}

#[derive(Clone, Copy, PartialEq)]
enum Drive {
    Vsync,
    Draw,
}

struct Bench {
    drive: Drive,
    window: gpui::AnyWindowHandle,
    pane: Entity<DiffPane>,
    repo: PathBuf,
    queue: VecDeque<Phase>,
    run: Option<Run>,
    scroll: Map<String, Value>,
    launch: Value,
}

fn plan() -> VecDeque<Phase> {
    use DiffMode::{Split, Unified};
    let phase = |name, mode, wrap, frames, step| Phase {
        name,
        mode,
        wrap,
        frames,
        step,
    };
    let flick = || Step::Pixels(100.0);
    VecDeque::from([
        phase("sweep_nowrap", Unified, false, SWEEP_FRAMES, Step::Sweep),
        phase("sweep_wrap", Unified, true, SWEEP_FRAMES, Step::Sweep),
        phase("flick_nowrap", Unified, false, FLICK_FRAMES, flick()),
        phase("flick_wrap", Unified, true, FLICK_FRAMES, flick()),
        phase("split_sweep_nowrap", Split, false, SWEEP_FRAMES, Step::Sweep),
        phase("split_sweep_wrap", Split, true, SWEEP_FRAMES, Step::Sweep),
        phase("split_flick_nowrap", Split, false, FLICK_FRAMES, flick()),
        phase("split_flick_wrap", Split, true, FLICK_FRAMES, flick()),
    ])
}

fn start_phase(bench: &Rc<RefCell<Bench>>, window: &mut Window, cx: &mut App) {
    let next = bench.borrow_mut().queue.pop_front();
    let Some(phase) = next else {
        eprintln!("PHASE end");
        let bench = bench.clone();
        cx.spawn(async move |cx| {
            let repo = bench.borrow().repo.clone();
            let started = Instant::now();
            let retained = reopen_retained(&repo, cx).await;
            eprintln!("PHASE reopen took {:.0} ms", ms(started));
            let bench = bench.borrow();
            let nested = json!({
                "launch": bench.launch,
                "load": headless(&bench.repo, false),
                "scroll": Value::Object(bench.scroll.clone()),
                "pane_retained_bytes": retained,
            });
            let mut report = Map::new();
            flatten("", &nested, &mut report);
            println!("{}", serde_json::to_string_pretty(&report).expect("report serializes"));
            cx.update(|cx| cx.quit()).ok();
        })
        .detach();
        return;
    };
    eprintln!("PHASE {}", phase.name);
    let pane = bench.borrow().pane.clone();
    let step_px = pane.update(cx, |pane, cx| {
        pane.set_mode(phase.mode, cx);
        pane.set_wrap(phase.wrap, cx);
        pane.scroll_to(0.0, cx);
        pane.probe.enabled = true;
        pane.probe.frames_ms.clear();
        pane.probe.lines_ms.clear();
        match phase.step {
            Step::Sweep => (pane.state().tops.total() - pane.state().geometry().height) / phase.frames as f32,
            Step::Pixels(step) => step,
        }
    });
    bench.borrow_mut().run = Some(Run {
        phase,
        step_px,
        done: 0,
        last_tick: None,
        intervals: Vec::new(),
        meter: Meter::start(),
    });
    let drive = bench.borrow().drive;
    match drive {
        Drive::Vsync => schedule(bench.clone(), window),
        Drive::Draw => schedule_draw(bench.clone(), cx),
    }
}

fn schedule(bench: Rc<RefCell<Bench>>, window: &mut Window) {
    window.on_next_frame(move |window, cx| tick(&bench, window, cx));
}

fn schedule_draw(bench: Rc<RefCell<Bench>>, cx: &mut App) {
    let window = bench.borrow().window;
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_micros(100)).await;
        cx.update(|cx| {
            window
                .update(cx, |_, window, cx| {
                    window.draw(cx).clear();
                    tick(&bench, window, cx);
                })
                .ok();
        })
        .ok();
    })
    .detach();
}

fn tick(bench: &Rc<RefCell<Bench>>, window: &mut Window, cx: &mut App) {
    let pane = bench.borrow().pane.clone();
    let now = Instant::now();
    let finished = {
        let mut guard = bench.borrow_mut();
        let run = guard.run.as_mut().expect("a phase is running");
        run.meter.sample();
        if let Some(last) = run.last_tick.replace(now) {
            run.intervals.push(now.duration_since(last).as_secs_f32() * 1000.0);
        }
        run.done += 1;
        run.done > run.phase.frames
    };
    if finished {
        finish_phase(bench, window, cx);
        return;
    }
    let step = bench.borrow().run.as_ref().map_or(0.0, |run| run.step_px);
    pane.update(cx, |pane, cx| pane.scroll_by(step, 0.0, cx));
    let drive = bench.borrow().drive;
    match drive {
        Drive::Vsync => schedule(bench.clone(), window),
        Drive::Draw => schedule_draw(bench.clone(), cx),
    }
}

fn finish_phase(bench: &Rc<RefCell<Bench>>, window: &mut Window, cx: &mut App) {
    let pane = bench.borrow().pane.clone();
    let run = bench.borrow_mut().run.take().expect("a phase is running");
    let (cpu, lines, shown, rows) = pane.update(cx, |pane, _| {
        pane.probe.enabled = false;
        (
            std::mem::take(&mut pane.probe.frames_ms),
            std::mem::take(&mut pane.probe.lines_ms),
            pane.state().shown_mode(),
            pane.state().rows.rows.len(),
        )
    });
    eprintln!(
        "PHASE {} requested={} ticks={} painted={} shown={shown:?} rows={rows}",
        run.phase.name,
        run.phase.frames,
        run.intervals.len() + 1,
        cpu.len()
    );
    let mut metrics = Map::new();
    stats("interval", &run.intervals, &mut metrics);
    stats("cpu", &cpu, &mut metrics);
    stats("lines", &lines, &mut metrics);
    for (name, value) in run.meter.finish() {
        metrics.insert(name.into(), json!(value));
    }
    bench
        .borrow_mut()
        .scroll
        .insert(run.phase.name.to_string(), Value::Object(metrics));
    start_phase(bench, window, cx);
}

/// Opens a hidden window on the diff, lets highlighting finish, draws it, and closes it.
/// The first cycle fills the caches that live for the whole process, so it does not count.
async fn reopen_retained(repo: &Path, cx: &mut AsyncApp) -> i64 {
    let mut before = 0;
    for cycle in 0..=REOPEN_CYCLES {
        let (files, _) = load(repo);
        let window = cx
            .update(|cx| {
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(WINDOW.0), px(WINDOW.1)),
                    ))),
                    show: false,
                    focus: false,
                    ..WindowOptions::default()
                };
                cx.open_window(options, |window, cx| {
                    diffpilot_ui_kit::theme::install(window, cx);
                    let pane = cx.new(|cx| DiffPane::new(window, cx));
                    pane.update(cx, |pane, cx| pane.set_files(files, cx));
                    pane
                })
            })
            .expect("app is alive")
            .expect("window opens");
        for _ in 0..40 {
            cx.background_executor().timer(Duration::from_millis(50)).await;
            if !window.read_with(cx, |pane, _| pane.is_highlighting()).unwrap_or(false) {
                break;
            }
        }
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.remove_window();
        })
        .ok();
        cx.background_executor().timer(Duration::from_millis(50)).await;
        if cycle == 0 {
            before = live_bytes();
        }
    }
    live_bytes() - before
}

fn start_bench(
    pane: &Entity<DiffPane>,
    repo: PathBuf,
    launch: Value,
    drive: Drive,
    window: gpui::AnyWindowHandle,
    cx: &mut App,
) {
    let bench = Rc::new(RefCell::new(Bench {
        drive,
        window,
        pane: pane.clone(),
        repo,
        queue: plan(),
        run: None,
        scroll: Map::new(),
        launch,
    }));
    let pane = pane.clone();
    cx.spawn(async move |cx| {
        for _ in 0..40 {
            cx.background_executor().timer(Duration::from_millis(50)).await;
            if !pane.read_with(cx, |pane, _| pane.is_highlighting()).unwrap_or(false) {
                break;
            }
        }
        cx.update(|cx| {
            window.update(cx, |_, window, cx| start_phase(&bench, window, cx)).ok();
        })
        .ok();
    })
    .detach();
}

/// A covered window gets no frame callbacks, so a bench window floats above the others.
fn bench_window_kind(benching: bool) -> WindowKind {
    if benching {
        WindowKind::PopUp
    } else {
        WindowKind::Normal
    }
}

fn demo_enabled() -> bool {
    std::env::var("DIFFPILOT_DEMO_THREADS").is_ok_and(|value| value == "1")
}

/// Comments on the first changed line pair the fixture has, one on each side, for screenshots.
fn demo_threads(files: &[FileDiff], only: Option<&str>) -> Option<(String, Vec<(ThreadAnchor, &'static str)>)> {
    files.iter().filter(|file| only.is_none_or(|path| file.path == path)).find_map(|file| {
        file.hunks.iter().find_map(|hunk| {
            let at = hunk
                .lines
                .windows(2)
                .position(|pair| pair[0].kind == LineKind::Deleted && pair[1].kind == LineKind::Added)?;
            let (old, new) = (hunk.lines[at].old?, hunk.lines[at + 1].new?);
            Some((
                file.path.clone(),
                vec![
                    (
                        ThreadAnchor::line(&file.path, Side::Old, old),
                        "This line is gone. Was it covered by a test?",
                    ),
                    (
                        ThreadAnchor::line(&file.path, Side::New, new),
                        "The new name reads better. Please add a short note on why the order matters here, because the next person will wonder.",
                    ),
                ],
            ))
        })
    })
}

fn run_window(repo: PathBuf, launch: Instant, bench: Option<Drive>) {
    let demo = demo_enabled();
    Application::new().with_assets(Assets).run(move |cx: &mut App| {
        diffpilot_ui_kit::init(cx);
        if demo {
            diffpilot::review::init(cx);
        }
        let slot: Rc<RefCell<Option<Entity<DiffPane>>>> = Rc::default();
        let review: Rc<RefCell<Option<Entity<ReviewSession>>>> = Rc::default();
        let displays = cx.displays();
        for (index, display) in displays.iter().enumerate() {
            eprintln!("display {index}: {:?}", display.bounds());
        }
        let display = std::env::var("DIFFPILOT_DISPLAY")
            .ok()
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| displays.get(index))
            .map(|display| display.id());
        let bounds = Bounds::centered(display, size(px(WINDOW.0), px(WINDOW.1)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            kind: bench_window_kind(bench.is_some()),
            ..WindowOptions::default()
        };
        let make_pane = {
            let (slot, review, repo) = (slot.clone(), review.clone(), repo.clone());
            move |window: &mut Window, cx: &mut App| {
                diffpilot_ui_kit::theme::install(window, cx);
                let pane = cx.new(|cx| DiffPane::new(window, cx));
                if let Some(mode) = settings::diff_mode_override() {
                    pane.update(cx, |pane, cx| pane.set_mode(mode, cx));
                }
                if demo {
                    let host = ReviewHost::new(Some(repo.clone()), Comparison::checked_out("main", true));
                    let session = cx.new(|cx| {
                        ReviewSession::new(
                            host,
                            Arc::new(Settings::in_memory()),
                            Arc::new(Store::open_in_memory().expect("an in-memory store opens")),
                            cx,
                        )
                    });
                    pane.update(cx, |pane, cx| pane.set_review(session.clone(), window, cx));
                    *review.borrow_mut() = Some(session);
                }
                *slot.borrow_mut() = Some(pane.clone());
                pane
            }
        };
        let handle: AnyWindowHandle = if demo {
            cx.open_window(options, move |window, cx| {
                let pane = make_pane(window, cx);
                cx.new(|cx| Root::new(pane, window, cx))
            })
            .expect("window opens")
            .into()
        } else {
            cx.open_window(options, make_pane).expect("window opens").into()
        };
        let pane = slot.borrow().clone().expect("pane was created");
        let timeline: Rc<RefCell<Map<String, Value>>> = Rc::default();
        timeline.borrow_mut().insert("window_open_ms".into(), json!(ms(launch)));

        let viewed = Rc::new(RefCell::new(viewed_seed(&repo)));
        cx.subscribe(&pane, {
            let timeline = timeline.clone();
            let repo = repo.clone();
            let viewed = viewed.clone();
            move |pane, event: &DiffPaneEvent, cx| match event {
                DiffPaneEvent::FirstPaint => {
                    timeline.borrow_mut().insert("first_paint_ms".into(), json!(ms(launch)));
                    if bench == Some(Drive::Vsync) {
                        let launch = Value::Object(timeline.borrow().clone());
                        start_bench(&pane, repo.clone(), launch, Drive::Vsync, handle, cx);
                    }
                }
                DiffPaneEvent::VisibleFile(_)
                | DiffPaneEvent::ShownMode(_)
                | DiffPaneEvent::OpenInEditor { .. }
                | DiffPaneEvent::OpenRef(_)
                | DiffPaneEvent::RevealInFinder(_)
                | DiffPaneEvent::ThreadSteps
                | DiffPaneEvent::Changed => {}
                DiffPaneEvent::ToggleViewed(path) => {
                    let mut viewed = viewed.borrow_mut();
                    if !viewed.remove(path) {
                        viewed.insert(path.clone());
                    }
                    let next = viewed.clone();
                    pane.update(cx, |pane, cx| pane.set_viewed(next, cx));
                }
            }
        })
        .detach();

        let loading = cx.background_executor().spawn({
            let repo = repo.clone();
            async move { load(&repo) }
        });
        cx.spawn(async move |cx| {
            let (files, _) = loading.await;
            let loaded_ms = ms(launch);
            let reveal = std::env::var("DIFFPILOT_REVEAL").ok();
            let threads = if demo {
                demo_threads(&files, reveal.as_deref())
            } else {
                None
            };
            pane.update(cx, |pane, cx| {
                let started = Instant::now();
                pane.set_files(files, cx);
                if preview_enabled() {
                    pane.set_preview_source(Some((repo.clone(), Comparison::checked_out("main", true))), cx);
                }
                pane.set_viewed(viewed.borrow().clone(), cx);
                if std::env::var("DIFFPILOT_WRAP").is_ok_and(|value| value == "0") {
                    pane.set_wrap(false, cx);
                }
                if let Some(path) = &reveal {
                    pane.reveal(path, cx);
                }
                if let Some(offset) = std::env::var("DIFFPILOT_SCROLL")
                    .ok()
                    .and_then(|text| text.parse().ok())
                {
                    pane.scroll_to(offset, cx);
                }
                let mut timeline = timeline.borrow_mut();
                timeline.insert("diff_loaded_ms".into(), json!(loaded_ms));
                timeline.insert("set_files_ms".into(), json!(ms(started)));
            })
            .ok();
            if let (Some((path, threads)), Some(session)) = (threads, review.borrow().clone()) {
                session
                    .update(cx, |session, cx| {
                        for (anchor, text) in threads {
                            let id = session.create_thread(anchor, cx);
                            session.post(id, text, cx);
                        }
                    })
                    .ok();
                pane.update(cx, |pane, cx| pane.reveal(&path, cx)).ok();
            }
            if bench == Some(Drive::Draw) {
                let launch = Value::Object(timeline.borrow().clone());
                cx.update(|cx| start_bench(&pane, repo, launch, Drive::Draw, handle, cx))
                    .ok();
            }
        })
        .detach();
        cx.activate(true);
    });
}

fn main() {
    let launch = Instant::now();
    let repo = if preview_enabled() { preview_repo() } else { repo_path() };
    if std::env::var_os("DIFFPILOT_BENCH").is_some() {
        Counting::enable();
    }
    match std::env::var("DIFFPILOT_BENCH").as_deref() {
        Ok("load") => println!(
            "{}",
            serde_json::to_string_pretty(&headless(&repo, true)).expect("report serializes")
        ),
        Ok("draw") => run_window(repo, launch, Some(Drive::Draw)),
        Ok(_) => run_window(repo, launch, Some(Drive::Vsync)),
        Err(_) => run_window(repo, launch, None),
    }
}
