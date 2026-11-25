use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::{self, FileDiff};
use diffpilot_core::git;
use diffpilot_diff_pane::highlight;
use diffpilot_diff_pane::state::{Geometry, PaneState};
use diffpilot_diff_pane::{DiffPane, DiffPaneEvent};
use gpui::{App, AppContext as _, Application, Bounds, Entity, Window, WindowBounds, WindowOptions, px, size};
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const BUDGET_MS: f32 = 1000.0 / 120.0;
const WINDOW: (f32, f32) = (1200.0, 800.0);

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
}

struct Bench {
    pane: Entity<DiffPane>,
    repo: PathBuf,
    queue: VecDeque<Phase>,
    run: Option<Run>,
    scroll: Map<String, Value>,
    launch: Value,
}

fn plan() -> VecDeque<Phase> {
    VecDeque::from([
        Phase {
            name: "sweep_nowrap",
            wrap: false,
            frames: 800,
            step: Step::Sweep,
        },
        Phase {
            name: "sweep_wrap",
            wrap: true,
            frames: 800,
            step: Step::Sweep,
        },
        Phase {
            name: "flick_nowrap",
            wrap: false,
            frames: 240,
            step: Step::Pixels(100.0),
        },
        Phase {
            name: "flick_wrap",
            wrap: true,
            frames: 240,
            step: Step::Pixels(100.0),
        },
    ])
}

fn start_phase(bench: &Rc<RefCell<Bench>>, window: &mut Window, cx: &mut App) {
    let next = bench.borrow_mut().queue.pop_front();
    let Some(phase) = next else {
        eprintln!("PHASE end");
        let bench = bench.borrow();
        let nested = json!({
            "launch": bench.launch,
            "load": headless(&bench.repo, false),
            "scroll": Value::Object(bench.scroll.clone()),
        });
        let mut report = Map::new();
        flatten("", &nested, &mut report);
        println!("{}", serde_json::to_string_pretty(&report).expect("report serializes"));
        cx.quit();
        return;
    };
    eprintln!("PHASE {}", phase.name);
    let pane = bench.borrow().pane.clone();
    let step_px = pane.update(cx, |pane, cx| {
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
    });
    schedule(bench.clone(), window);
}

fn schedule(bench: Rc<RefCell<Bench>>, window: &mut Window) {
    window.on_next_frame(move |window, cx| tick(&bench, window, cx));
}

fn tick(bench: &Rc<RefCell<Bench>>, window: &mut Window, cx: &mut App) {
    let now = Instant::now();
    let pane = bench.borrow().pane.clone();
    let finished = {
        let mut guard = bench.borrow_mut();
        let run = guard.run.as_mut().expect("a phase is running");
        if let Some(last) = run.last_tick.replace(now) {
            run.intervals.push(now.duration_since(last).as_secs_f32() * 1000.0);
        }
        run.done += 1;
        run.done > run.phase.frames
    };
    if finished {
        let run = bench.borrow_mut().run.take().expect("a phase is running");
        let (cpu, lines) = pane.update(cx, |pane, _| {
            pane.probe.enabled = false;
            (
                std::mem::take(&mut pane.probe.frames_ms),
                std::mem::take(&mut pane.probe.lines_ms),
            )
        });
        eprintln!(
            "PHASE {} requested={} ticks={} painted={}",
            run.phase.name,
            run.phase.frames,
            run.intervals.len() + 1,
            cpu.len()
        );
        let mut metrics = Map::new();
        stats("interval", &run.intervals, &mut metrics);
        stats("cpu", &cpu, &mut metrics);
        stats("lines", &lines, &mut metrics);
        bench
            .borrow_mut()
            .scroll
            .insert(run.phase.name.to_string(), Value::Object(metrics));
        start_phase(bench, window, cx);
        return;
    }
    let step = bench.borrow().run.as_ref().map_or(0.0, |run| run.step_px);
    pane.update(cx, |pane, cx| pane.scroll_by(step, 0.0, cx));
    schedule(bench.clone(), window);
}

fn start_bench(pane: &Entity<DiffPane>, repo: PathBuf, launch: Value, window: gpui::AnyWindowHandle, cx: &mut App) {
    let bench = Rc::new(RefCell::new(Bench {
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

fn run_window(repo: PathBuf, launch: Instant, bench_mode: bool) {
    Application::new().run(move |cx: &mut App| {
        let slot: Rc<RefCell<Option<Entity<DiffPane>>>> = Rc::default();
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
            ..WindowOptions::default()
        };
        let handle = cx
            .open_window(options, {
                let slot = slot.clone();
                move |window, cx| {
                    let pane = cx.new(|cx| DiffPane::new(window, cx));
                    match std::env::var("DIFFPILOT_APPEARANCE").as_deref() {
                        Ok("dark") => pane.update(cx, |pane, cx| pane.set_dark(Some(true), cx)),
                        Ok("light") => pane.update(cx, |pane, cx| pane.set_dark(Some(false), cx)),
                        _ => {}
                    }
                    *slot.borrow_mut() = Some(pane.clone());
                    pane
                }
            })
            .expect("window opens");
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
                    if bench_mode {
                        let launch = Value::Object(timeline.borrow().clone());
                        start_bench(&pane, repo.clone(), launch, handle.into(), cx);
                    }
                }
                DiffPaneEvent::VisibleFile(_) => {}
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
            pane.update(cx, |pane, cx| {
                let started = Instant::now();
                pane.set_files(files, cx);
                pane.set_viewed(viewed.borrow().clone(), cx);
                let mut timeline = timeline.borrow_mut();
                timeline.insert("diff_loaded_ms".into(), json!(loaded_ms));
                timeline.insert("set_files_ms".into(), json!(ms(started)));
            })
            .ok();
        })
        .detach();
        cx.activate(true);
    });
}

fn main() {
    let launch = Instant::now();
    let repo = repo_path();
    match std::env::var("DIFFPILOT_BENCH").as_deref() {
        Ok("load") => println!(
            "{}",
            serde_json::to_string_pretty(&headless(&repo, true)).expect("report serializes")
        ),
        Ok(_) => run_window(repo, launch, true),
        Err(_) => run_window(repo, launch, false),
    }
}
