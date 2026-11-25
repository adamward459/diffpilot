use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels,
    Window, point, px,
};
use gpui::{Global, UniformListScrollHandle};
use gpui_component::scroll::ScrollbarHandle;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;

pub const FRAME_BUDGET_MS: f64 = 1000.0 / 120.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub frames: usize,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
    pub over_budget: usize,
}

pub fn stats(samples: &[f64], budget: f64) -> Stats {
    if samples.is_empty() {
        return Stats {
            frames: 0,
            median: 0.0,
            p95: 0.0,
            max: 0.0,
            over_budget: 0,
        };
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |fraction: f64| sorted[((sorted.len() - 1) as f64 * fraction).round() as usize];
    Stats {
        frames: sorted.len(),
        median: at(0.5),
        p95: at(0.95),
        max: *sorted.last().expect("not empty"),
        over_budget: sorted.iter().filter(|&&value| value > budget).count(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    WindowShown,
    SidebarLoaded,
    FirstDiffPaint,
}

impl Mark {
    fn key(self) -> &'static str {
        match self {
            Mark::WindowShown => "window_shown_ms",
            Mark::SidebarLoaded => "sidebar_loaded_ms",
            Mark::FirstDiffPaint => "first_diff_paint_ms",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub pixels_per_frame: f32,
}

impl Config {
    pub fn from_env() -> Option<Self> {
        Self::parse(
            std::env::var("DIFFPILOT_BENCH").ok().as_deref(),
            std::env::var("DIFFPILOT_BENCH_PX").ok().as_deref(),
        )
    }

    pub fn parse(bench: Option<&str>, pixels: Option<&str>) -> Option<Self> {
        let enabled = bench.is_some_and(|value| !value.is_empty() && value != "0");
        enabled.then(|| Self {
            pixels_per_frame: pixels
                .and_then(|value| value.parse().ok())
                .filter(|px| *px > 0.0)
                .unwrap_or(96.0),
        })
    }
}

struct Scroll {
    handle: UniformListScrollHandle,
    offset: f32,
    last_frame: Option<Instant>,
    frame_start: Option<Instant>,
    intervals: Vec<f64>,
    work: Vec<f64>,
    rows: usize,
}

struct Inner {
    start: Instant,
    config: Config,
    marks: BTreeMap<Mark, f64>,
    stages: BTreeMap<&'static str, f64>,
    after_paint: Vec<Mark>,
    scroll: Option<Scroll>,
    scroll_started: bool,
    finished: bool,
}

#[derive(Clone)]
pub struct Bench(Rc<RefCell<Inner>>);

impl Global for Bench {}

impl Bench {
    pub fn install(cx: &mut App, start: Instant, config: Config) {
        cx.set_global(Bench(Rc::new(RefCell::new(Inner {
            start,
            config,
            marks: BTreeMap::new(),
            stages: BTreeMap::new(),
            after_paint: Vec::new(),
            scroll: None,
            scroll_started: false,
            finished: false,
        }))));
    }

    pub fn get(cx: &App) -> Option<Bench> {
        cx.try_global::<Bench>().cloned()
    }

    pub fn mark_after_paint(cx: &App, mark: Mark) {
        if let Some(bench) = Self::get(cx) {
            let mut inner = bench.0.borrow_mut();
            if !inner.marks.contains_key(&mark) && !inner.after_paint.contains(&mark) {
                inner.after_paint.push(mark);
            }
        }
    }

    pub fn note_stage(cx: &App, name: &'static str, elapsed: std::time::Duration) {
        if let Some(bench) = Self::get(cx) {
            bench
                .0
                .borrow_mut()
                .stages
                .entry(name)
                .or_insert(elapsed.as_secs_f64() * 1000.0);
        }
    }

    pub fn mark_now(cx: &App, mark: Mark) {
        if let Some(bench) = Self::get(cx) {
            let mut inner = bench.0.borrow_mut();
            let elapsed = inner.start.elapsed().as_secs_f64() * 1000.0;
            inner.marks.entry(mark).or_insert(elapsed);
        }
    }

    pub fn ready_to_scroll(&self) -> bool {
        let inner = self.0.borrow();
        !inner.scroll_started
            && inner.marks.contains_key(&Mark::SidebarLoaded)
            && inner.marks.contains_key(&Mark::FirstDiffPaint)
    }

    pub fn begin_scroll(&self, handle: UniformListScrollHandle, rows: usize) {
        let mut inner = self.0.borrow_mut();
        inner.scroll_started = true;
        inner.scroll = Some(Scroll {
            handle,
            offset: 0.0,
            last_frame: None,
            frame_start: None,
            intervals: Vec::new(),
            work: Vec::new(),
            rows,
        });
    }

    pub fn on_paint(&self) {
        let mut inner = self.0.borrow_mut();
        let elapsed = inner.start.elapsed().as_secs_f64() * 1000.0;
        for mark in std::mem::take(&mut inner.after_paint) {
            inner.marks.entry(mark).or_insert(elapsed);
        }
        if let Some(scroll) = inner.scroll.as_mut()
            && let Some(frame_start) = scroll.frame_start.take()
        {
            scroll.work.push(frame_start.elapsed().as_secs_f64() * 1000.0);
        }
    }

    fn step(&self, window: &mut Window, cx: &mut App) {
        let done = {
            let mut inner = self.0.borrow_mut();
            let step_px = inner.config.pixels_per_frame;
            let Some(scroll) = inner.scroll.as_mut() else { return };
            let now = Instant::now();
            if let Some(last) = scroll.last_frame {
                scroll.intervals.push((now - last).as_secs_f64() * 1000.0);
            }
            scroll.last_frame = Some(now);
            scroll.frame_start = Some(now);
            let max = scroll.handle.0.borrow().base_handle.max_offset().height;
            scroll.offset += step_px;
            let at_end = px(scroll.offset) >= max;
            scroll
                .handle
                .set_offset(point(px(0.0), -px(scroll.offset.min(f32::from(max)))));
            at_end
        };
        if done {
            self.finish(cx);
            return;
        }
        window.refresh();
        let bench = self.clone();
        window.on_next_frame(move |window, cx| bench.step(window, cx));
    }

    pub fn run_scroll(&self, window: &mut Window) {
        let bench = self.clone();
        window.refresh();
        window.on_next_frame(move |window, cx| bench.step(window, cx));
    }

    fn finish(&self, cx: &mut App) {
        let report = {
            let mut inner = self.0.borrow_mut();
            if inner.finished {
                return;
            }
            inner.finished = true;
            let scroll = inner.scroll.take().expect("scroll data");
            let intervals = stats(&scroll.intervals, FRAME_BUDGET_MS);
            let work = stats(&scroll.work, FRAME_BUDGET_MS);
            report_json(
                &inner.marks,
                &inner.stages,
                &intervals,
                &work,
                scroll.rows,
                inner.config.pixels_per_frame,
            )
        };
        println!("{report}");
        cx.quit();
    }
}

fn number(value: f64) -> String {
    format!("{value:.2}")
}

fn stats_json(stats: &Stats) -> String {
    format!(
        "{{\"frames\": {}, \"median_ms\": {}, \"p95_ms\": {}, \"max_ms\": {}, \"frames_over_8_33_ms\": {}}}",
        stats.frames,
        number(stats.median),
        number(stats.p95),
        number(stats.max),
        stats.over_budget
    )
}

pub fn report_json(
    marks: &BTreeMap<Mark, f64>,
    stages: &BTreeMap<&'static str, f64>,
    intervals: &Stats,
    work: &Stats,
    rows: usize,
    pixels_per_frame: f32,
) -> String {
    let mut fields: Vec<String> = [Mark::WindowShown, Mark::SidebarLoaded, Mark::FirstDiffPaint]
        .iter()
        .map(|mark| match marks.get(mark) {
            Some(value) => format!("  \"{}\": {}", mark.key(), number(*value)),
            None => format!("  \"{}\": null", mark.key()),
        })
        .collect();
    let stage_fields: Vec<String> = stages
        .iter()
        .map(|(name, value)| format!("\"{name}\": {}", number(*value)))
        .collect();
    fields.push(format!("  \"stages_ms\": {{{}}}", stage_fields.join(", ")));
    fields.push(format!("  \"sidebar_rows\": {rows}"));
    fields.push(format!("  \"scroll_pixels_per_frame\": {pixels_per_frame}"));
    fields.push(format!("  \"scroll_frame_interval\": {}", stats_json(intervals)));
    fields.push(format!("  \"scroll_frame_cpu\": {}", stats_json(work)));
    format!("{{\n{}\n}}", fields.join(",\n"))
}

impl IntoElement for ProbeElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub struct ProbeElement;

impl Element for ProbeElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (window.request_layout(gpui::Style::default(), [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        _: &mut Window,
        cx: &mut App,
    ) {
        if let Some(bench) = Bench::get(cx) {
            bench.on_paint();
        }
    }
}

pub fn probe() -> AnyElement {
    ProbeElement.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_summarize_frame_times() {
        let samples: Vec<f64> = (1..=100).map(f64::from).collect();
        let result = stats(&samples, 8.33);
        assert_eq!(result.frames, 100);
        assert_eq!(result.median, 51.0);
        assert_eq!(result.p95, 95.0);
        assert_eq!(result.max, 100.0);
        assert_eq!(result.over_budget, 92);
    }

    #[test]
    fn empty_stats_are_zero() {
        assert_eq!(stats(&[], 8.33).frames, 0);
    }

    #[test]
    fn bench_mode_needs_a_non_empty_non_zero_value() {
        assert_eq!(Config::parse(None, None), None);
        assert_eq!(Config::parse(Some(""), None), None);
        assert_eq!(Config::parse(Some("0"), None), None);
        assert_eq!(Config::parse(Some("1"), None).unwrap().pixels_per_frame, 96.0);
        assert_eq!(Config::parse(Some("1"), Some("48")).unwrap().pixels_per_frame, 48.0);
        assert_eq!(Config::parse(Some("1"), Some("-3")).unwrap().pixels_per_frame, 96.0);
    }

    #[test]
    fn report_is_valid_looking_json_with_nulls_for_missing_marks() {
        let mut marks = BTreeMap::new();
        marks.insert(Mark::WindowShown, 12.5);
        let empty = stats(&[], FRAME_BUDGET_MS);
        let json = report_json(&marks, &BTreeMap::new(), &empty, &empty, 10, 96.0);
        assert!(json.contains("\"window_shown_ms\": 12.50"));
        assert!(json.contains("\"sidebar_loaded_ms\": null"));
        assert!(json.contains("\"frames_over_8_33_ms\": 0"));
        assert!(json.starts_with('{') && json.ends_with('}'));
    }
}
