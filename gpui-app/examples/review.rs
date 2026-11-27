//! Shows the review pill and a few thread cards.
//!
//! `DIFFPILOT_REVIEW_SECONDS` sets how long the fake `claude` takes to review.
//! `DIFFPILOT_APPEARANCE=light|dark` picks the theme.
//! `DIFFPILOT_DEMO_START=1` starts a review at launch. `DIFFPILOT_DEMO_POPOVER=1` opens the popover.
//! `DIFFPILOT_BENCH=1` runs a hidden window and prints frame times as JSON on stdout.

use diffpilot::bench::{Counting, Meter, live_bytes};
use diffpilot::review::{PillEvent, ReviewEvent, ReviewHost, ReviewPill, ReviewSession, ThreadCard, ThreadChange};
use diffpilot::runs::Runs;
use diffpilot::testing::{FakeClaude, fake_engine};
use diffpilot_core::changes::FileStatus;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::{DiffLine, FileDiff, Hunk, LineKind};
use diffpilot_core::review::{Finding, ReviewResult, Severity};
use diffpilot_core::run::{RunKind, RunState};
use diffpilot_core::settings::Settings;
use diffpilot_core::store::Store;
use diffpilot_core::threads::{Side, ThreadAnchor, ThreadId};
use diffpilot_ui_kit::components::scroll_y;
use gpui::{
    App, AppContext, Application, AsyncApp, Bounds, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, Styled, Subscription, Window, WindowBounds, WindowOptions, canvas, div, point, px, size,
};
use gpui_component::{ActiveTheme, Root, h_flex, v_flex};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const CARD_WIDTH: f32 = 720.0;
const REOPEN_CYCLES: usize = 5;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

type Heights = Rc<RefCell<HashMap<ThreadId, Pixels>>>;

fn archive_file() -> FileDiff {
    let mut file = FileDiff::new("docs/archive.md", FileStatus::Added);
    file.additions = 19;
    file.hunks.push(Hunk {
        header: "@@ -0,0 +1,19 @@".into(),
        lines: (0..19)
            .map(|n| DiffLine {
                kind: LineKind::Added,
                old: None,
                new: Some(n + 1),
                text: format!("docs/archive.md new {n}"),
            })
            .collect(),
    });
    file
}

struct Gallery {
    scroll: gpui::ScrollHandle,
    pill: Entity<ReviewPill>,
    cards: Vec<Entity<ThreadCard>>,
    heights: Heights,
    room: bool,
    show_cards: bool,
    _subscriptions: Vec<Subscription>,
}

impl Gallery {
    fn new(session: Entity<ReviewSession>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pill = cx.new(|cx| ReviewPill::new(session.clone(), window, cx));
        let subscriptions = vec![
            cx.subscribe(&session, |this, session, event, cx| match event {
                ReviewEvent::ThreadsChanged(ThreadChange::Added(id)) => {
                    let card = cx.new(|cx| ThreadCard::new(session.clone(), *id, cx));
                    this.cards.push(card);
                    cx.notify();
                }
                ReviewEvent::ThreadsChanged(ThreadChange::Removed(id)) => {
                    this.cards.retain(|card| card.read(cx).thread_id() != *id);
                    cx.notify();
                }
                _ => {}
            }),
            cx.subscribe(&pill, |this, _, event, cx| match event {
                PillEvent::NeedsRoomChanged(room) => {
                    this.room = *room;
                    cx.notify();
                }
                PillEvent::ShowArea => {}
            }),
        ];
        Self {
            scroll: gpui::ScrollHandle::new(),
            pill,
            cards: Vec::new(),
            heights: Heights::default(),
            room: false,
            show_cards: true,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, border, muted) = (theme.background, theme.border, theme.muted_foreground);
        let room = if self.room { "pill has room" } else { "compact" };
        v_flex()
            .size_full()
            .bg(background)
            .child(
                h_flex()
                    .h(px(44.))
                    .px_4()
                    .gap_4()
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .child(div().text_color(muted).child("large"))
                    .child(self.pill.clone())
                    .child(
                        div()
                            .text_color(muted)
                            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                            .child(room),
                    ),
            )
            .child(
                scroll_y(
                    &self.scroll,
                    v_flex()
                        .id("cards")
                        .p_4()
                        .gap_3()
                        .when_cards(self.show_cards, |column| {
                            column.children(self.cards.iter().map(|card| {
                                let id = card.read(cx).thread_id();
                                let heights = self.heights.clone();
                                div().relative().w(px(CARD_WIDTH)).child(card.clone()).child(
                                    canvas(
                                        move |bounds: Bounds<Pixels>, _, _| {
                                            heights.borrow_mut().insert(id, bounds.size.height);
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .size_full(),
                                )
                            }))
                        }),
                )
                .flex_1(),
            )
    }
}

trait WhenCards: Sized {
    fn when_cards(self, show: bool, build: impl FnOnce(Self) -> Self) -> Self {
        if show { build(self) } else { self }
    }
}
impl<T> WhenCards for T {}

fn demo_review() -> ReviewResult {
    let finding = |path: &str, line: Option<u32>, severity, title: &str, body: &str| Finding {
        path: path.into(),
        line,
        severity,
        title: title.into(),
        body: body.into(),
    };
    ReviewResult {
        summary: "The archive flow works, but one path skips the undo step.".into(),
        findings: vec![
            finding(
                "docs/archive.md",
                Some(2),
                Severity::MustFix,
                "Undo window is not documented",
                "The doc names no undo window. Add the time limit in `docs/archive.md:2`.",
            ),
            finding(
                "src/api/archive.ts",
                Some(9999),
                Severity::Suggestion,
                "Retry is missing",
                "A failed archive call is not retried. Wrap it in the shared retry helper.",
            ),
            finding(
                "src/api/client.ts",
                None,
                Severity::Nit,
                "Name is vague",
                "Rename `req` to `request`.",
            ),
            finding(
                "scripts/missing.py",
                Some(3),
                Severity::Question,
                "Is this file still used?",
                "Nothing in the diff calls this script. Should it stay?",
            ),
        ],
    }
}

const DEMO_REPLY: &str = "I read the change around this line. The `onDismiss` callback now runs after the sheet animates out, so a caller that reads layout inside it sees the final frame. I would keep it, and add a test that dismisses twice in a row.";

/// With `seconds`, each run ends that long after it starts. Without, runs wait for `FakeClaude::finish`.
fn install_fake_claude(cx: &mut App, seconds: Option<f64>) -> &'static FakeClaude {
    let claude = fake_engine();
    claude.review_with(&demo_review());
    if let Some(seconds) = seconds {
        claude.finish_after(seconds);
    }
    cx.set_global(Runs::with_engine(claude.engine()));
    Box::leak(Box::new(claude))
}

/// Answers the reply run of `id` through the MCP tool, as Claude would, once that run has started.
async fn answer_when_asked(
    session: &Entity<ReviewSession>,
    claude: &FakeClaude,
    id: ThreadId,
    body: &str,
    cx: &mut AsyncApp,
) {
    loop {
        let started = session
            .read_with(cx, |session, cx| {
                session.activity().read(cx).current().is_some_and(|run| {
                    run.kind == RunKind::Reply && run.state == RunState::Running && run.threads.contains(&id)
                })
            })
            .unwrap_or(true);
        if started {
            break;
        }
        cx.background_executor().timer(Duration::from_millis(50)).await;
    }
    cx.background_executor().timer(Duration::from_millis(300)).await;
    claude.call_mcp_tool(
        "reply",
        serde_json::json!({ "threadId": id.0.to_string(), "body": body }),
    );
}

fn review_seconds() -> f64 {
    std::env::var("DIFFPILOT_REVIEW_SECONDS")
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(1.5)
}

/// Runs start `claude` in the repo folder, so it has to exist.
fn session(cx: &mut App, store: Arc<Store>) -> Entity<ReviewSession> {
    let repo = std::env::temp_dir().join("diffpilot-demo");
    std::fs::create_dir_all(&repo).expect("make the demo folder");
    let mut host = ReviewHost::new(Some(repo), Comparison::checked_out("main", true));
    host.branch_name = Some("feature".into());
    host.files = Arc::from([Arc::new(archive_file())]);
    host.visible_path = Some("docs/archive.md".into());
    cx.new(|cx| ReviewSession::new(host, Arc::new(Settings::in_memory()), store, cx))
}

fn open_gallery(
    cx: &mut App,
    hidden: bool,
    session: Entity<ReviewSession>,
) -> (gpui::WindowHandle<Root>, Entity<Gallery>) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(80.), px(80.)),
            size(px(CARD_WIDTH + 80.), px(900.)),
        ))),
        show: !hidden,
        focus: !hidden,
        ..WindowOptions::default()
    };
    let mut made = None;
    let window = cx
        .open_window(options, |window, cx| {
            diffpilot_ui_kit::theme::install(window, cx);
            let gallery = cx.new(|cx| Gallery::new(session, window, cx));
            made = Some(gallery.clone());
            cx.new(|cx| Root::new(gallery, window, cx))
        })
        .expect("open the window");
    (window, made.expect("the window builds its root"))
}

fn run_demo(cx: &mut App) {
    let claude = install_fake_claude(cx, Some(review_seconds()));
    let session = session(cx, claude.store());
    let (_, gallery) = open_gallery(cx, false, session.clone());
    let flag = |name: &str| std::env::var(name).is_ok_and(|value| value == "1");
    if flag("DIFFPILOT_DEMO_POPOVER") {
        gallery.update(cx, |gallery, cx| {
            gallery.pill.update(cx, |pill, cx| pill.set_open(true, cx))
        });
    }
    if flag("DIFFPILOT_DEMO_START") {
        session.update(cx, |session, cx| session.start_review(cx));
    }
    let asked = session.update(cx, |session, cx| {
        let id = session.create_thread(ThreadAnchor::line("docs/archive.md", Side::New, 3), cx);
        session.post(id, "@claude is this line needed?", cx);
        session.create_thread(ThreadAnchor::file("docs/archive.md"), cx);
        id
    });
    let answering = session.clone();
    cx.spawn(async move |cx| answer_when_asked(&answering, claude, asked, DEMO_REPLY, cx).await)
        .detach();
    cx.activate(true);
}

fn long_reply(words: usize) -> String {
    let sentence = "I read the change around this line. The `onDismiss` callback now runs after the sheet animates out, so a caller that reads layout inside it sees the final frame. I would keep it and add a test that dismisses twice in a row. ";
    sentence.split(' ').cycle().take(words).collect::<Vec<_>>().join(" ")
}

fn milliseconds(mut samples: Vec<Duration>) -> (f64, f64, f64) {
    samples.sort();
    let at = |fraction: f64| samples[((samples.len() - 1) as f64 * fraction).round() as usize].as_secs_f64() * 1000.0;
    (at(0.5), at(0.95), at(1.0))
}

async fn bench(cx: &mut AsyncApp) {
    let words: usize = std::env::var("DIFFPILOT_BENCH_WORDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(240);
    let (window, gallery, session, claude) = cx
        .update(|cx| {
            let claude = install_fake_claude(cx, None);
            let session = session(cx, claude.store());
            let (window, gallery) = open_gallery(cx, true, session.clone());
            (window, gallery, session, claude)
        })
        .expect("app is alive");
    let draw = |cx: &mut AsyncApp| {
        cx.update_window(window.into(), |_, window, cx| {
            let started = Instant::now();
            window.draw(cx).clear();
            started.elapsed()
        })
        .expect("window is alive")
    };
    for _ in 0..5 {
        draw(cx);
    }
    let baseline: Vec<Duration> = (0..60).map(|_| draw(cx)).collect();

    let id = session
        .update(cx, |session, cx| {
            let id = session.create_thread(ThreadAnchor::line("docs/archive.md", Side::New, 3), cx);
            session.post(id, "Is this line needed? Please explain in detail.", cx);
            id
        })
        .expect("app is alive");
    cx.background_executor().timer(Duration::from_millis(50)).await;
    for _ in 0..5 {
        draw(cx);
    }
    let mut meter = Meter::start();
    let idle: Vec<Duration> = (0..60)
        .map(|_| {
            let took = draw(cx);
            meter.sample();
            took
        })
        .collect();
    let idle_memory = meter.finish();

    session
        .update(cx, |session, cx| session.ask_claude(id, cx))
        .expect("app is alive");
    let mut stream = Vec::new();
    let mut chars = 0;
    let mut answered_at = None;
    let mut meter = Meter::start();
    while session
        .read_with(cx, |session, cx| session.is_replying(id, cx))
        .expect("app is alive")
    {
        cx.background_executor().timer(Duration::from_millis(33)).await;
        stream.push(draw(cx));
        meter.sample();
        let running = session
            .read_with(cx, |session, cx| {
                session
                    .activity()
                    .read(cx)
                    .current()
                    .is_some_and(|run| run.state == RunState::Running)
            })
            .expect("app is alive");
        match answered_at {
            None if running && stream.len() >= 10 => {
                claude.call_mcp_tool(
                    "reply",
                    serde_json::json!({ "threadId": id.0.to_string(), "body": long_reply(words) }),
                );
                answered_at = Some(stream.len());
            }
            Some(at) if stream.len() == at + 30 => claude.finish(),
            _ => {}
        }
        chars = session
            .read_with(cx, |session, _| {
                session
                    .thread(id)
                    .and_then(|t| t.comments.last())
                    .map_or(0, |c| c.body.len())
            })
            .expect("app is alive");
    }
    draw(cx);
    let stream_memory = meter.finish();

    session
        .update(cx, |session, cx| {
            session.start_review(cx);
            session.create_thread(ThreadAnchor::file("docs/archive.md"), cx);
        })
        .expect("app is alive");
    cx.background_executor().timer(Duration::from_millis(100)).await;
    cx.update_window(window.into(), |_, window, cx| {
        let cards = gallery.read(cx).cards.clone();
        for card in cards.iter().skip(2).take(2) {
            card.update(cx, |card, cx| card.focus_composer(window, cx));
        }
    })
    .expect("window is alive");
    for _ in 0..3 {
        draw(cx);
    }
    let cards = gallery
        .read_with(cx, |gallery, _| gallery.cards.clone())
        .expect("app is alive");
    let (errors, measure_cost) = cx
        .update_window(window.into(), |_, window, cx| {
            let heights = gallery.read(cx).heights.borrow().clone();
            let errors: Vec<(ThreadId, f32, f32)> = cards
                .iter()
                .map(|card| {
                    let id = card.read(cx).thread_id();
                    let measured = f32::from(card.read(cx).height(px(CARD_WIDTH), window, cx));
                    (id, measured, f32::from(heights.get(&id).copied().unwrap_or_default()))
                })
                .collect();
            let started = Instant::now();
            for _ in 0..100 {
                for card in &cards {
                    card.read(cx).height(px(CARD_WIDTH), window, cx);
                }
            }
            (errors, started.elapsed() / (100 * cards.len() as u32))
        })
        .expect("window is alive");
    let worst = errors
        .iter()
        .map(|(_, measured, rendered)| (measured - rendered).abs())
        .fold(0.0, f32::max);
    for (id, measured, rendered) in &errors {
        let shape = session.read_with(cx, |s, _| {
            s.thread(*id)
                .map(|t| (t.finding.as_ref().map(|f| f.severity), t.comments.len()))
        });
        eprintln!("card {id:?} {shape:?} measured={measured} rendered={rendered}");
    }

    let (base_median, ..) = milliseconds(baseline);
    let (idle_median, idle_p95, idle_max) = milliseconds(idle);
    let (stream_median, stream_p95, stream_max) = milliseconds(stream.clone());
    eprintln!("stream frames={} final_chars={chars}", stream.len());
    let retained = reopen_retained(cx).await;
    let memory = [("idle_frame", idle_memory), ("stream_frame", stream_memory)]
        .into_iter()
        .flat_map(|(scenario, fields)| fields.map(|(name, value)| (format!("{scenario}_{name}"), value as f64)))
        .chain([("gallery_retained_bytes".to_owned(), retained as f64)]);
    let mut pairs: Vec<(String, f64)> = [
        ("baseline_frame_median_ms", base_median),
        ("idle_frame_median_ms", idle_median),
        ("idle_frame_p95_ms", idle_p95),
        ("idle_frame_max_ms", idle_max),
        ("stream_frame_median_ms", stream_median),
        ("stream_frame_p95_ms", stream_p95),
        ("stream_frame_max_ms", stream_max),
        ("measure_us", measure_cost.as_secs_f64() * 1e6),
        ("measure_error_px", f64::from(worst)),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    pairs.extend(memory);
    println!("{}", serde_like(&pairs));
    cx.update(|cx| cx.quit()).ok();
}

/// Opens a hidden gallery on a fresh session, adds a thread, draws it, and closes it.
/// The first cycle fills the caches that live for the whole process, so it does not count.
async fn reopen_retained(cx: &mut AsyncApp) -> i64 {
    let mut before = 0;
    for cycle in 0..=REOPEN_CYCLES {
        let window = cx
            .update(|cx| {
                let session = session(cx, Arc::new(Store::open_in_memory().expect("an in-memory store opens")));
                session.update(cx, |session, cx| {
                    let id = session.create_thread(ThreadAnchor::line("docs/archive.md", Side::New, 3), cx);
                    session.post(id, "Is this line needed?", cx);
                });
                open_gallery(cx, true, session).0
            })
            .expect("app is alive");
        for _ in 0..3 {
            cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear())
                .expect("window is alive");
        }
        cx.update_window(window.into(), |_, window, _| window.remove_window())
            .expect("window is alive");
        cx.background_executor().timer(Duration::from_millis(50)).await;
        if cycle == 0 {
            before = live_bytes();
        }
    }
    live_bytes() - before
}

fn serde_like(pairs: &[(String, f64)]) -> String {
    let body: Vec<String> = pairs
        .iter()
        .map(|(name, value)| format!("\"{name}\": {value:.3}"))
        .collect();
    format!("{{{}}}", body.join(", "))
}

fn main() {
    let benching = std::env::var("DIFFPILOT_BENCH").is_ok_and(|value| value == "1");
    if benching {
        Counting::enable();
    }
    Application::new()
        .with_assets(diffpilot_ui_kit::Assets)
        .run(move |cx: &mut App| {
            diffpilot_ui_kit::init(cx);
            diffpilot::review::init(cx);
            if benching {
                cx.spawn(async move |cx| bench(cx).await).detach();
            } else {
                run_demo(cx);
            }
        });
}
