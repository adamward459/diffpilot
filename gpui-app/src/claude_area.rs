use crate::claude_activity::ClaudeActivity;
use crate::review::popover::PopoverEvent;
use crate::review::{ReviewPopover, ReviewSession};
use crate::runs::now_ms;
use diffpilot_core::run::{LogEntry, LogKind, Run, RunId, RunState, StepState, first_line};
use diffpilot_ui_kit::components::menu::{MenuItem, MenuSection, menu_button};
use diffpilot_ui_kit::components::popover;
use diffpilot_ui_kit::components::{Button, Look, glyph_icon, spinner, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Context, Corner, Entity, EventEmitter, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, Render, ScrollStrategy, SharedString, StatefulInteractiveElement, Styled, Subscription, Task,
    UniformListScrollHandle, Window, div, px, uniform_list,
};
use gpui_component::{ActiveTheme, h_flex, v_flex};
use std::ops::Range;
use std::time::Duration;

pub const HEIGHT: f32 = 312.0;
const BAR_HEIGHT: f32 = 30.0;
const FOOTER_HEIGHT: f32 = 30.0;
const LEFT_WIDTH: f32 = 360.0;
const ROW_HEIGHT: f32 = 22.0;
const PAST_RUNS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeAreaEvent {
    Close,
}

pub struct ClaudeArea {
    session: Entity<ReviewSession>,
    activity: Entity<ClaudeActivity>,
    popover: Entity<ReviewPopover>,
    queue_open: bool,
    /// A past run the user picked. None follows the current run.
    picked: Option<Run>,
    log_scroll: UniformListScrollHandle,
    log_follow: LogFollow,
    ticker: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ClaudeAreaEvent> for ClaudeArea {}

#[derive(Default)]
struct LogFollow {
    run: Option<RunId>,
    rows: usize,
    following: bool,
}

impl LogFollow {
    /// Follows while the list sits at the bottom. A scroll up stops it, and a scroll back down or another run resumes it.
    /// Returns whether the list should scroll to its last row.
    fn update(&mut self, run: Option<RunId>, rows: usize, at_bottom: bool) -> bool {
        if self.run != run {
            *self = Self {
                run,
                rows: 0,
                following: true,
            };
        } else {
            self.following = at_bottom;
        }
        let grew = rows > self.rows;
        self.rows = rows;
        self.following && grew
    }
}

impl ClaudeArea {
    pub fn new(session: Entity<ReviewSession>, popover: Entity<ReviewPopover>, cx: &mut Context<Self>) -> Self {
        let activity = session.read(cx).activity().clone();
        let subscriptions = vec![
            cx.observe(&session, |this, _, cx| this.sync(cx)),
            cx.observe(&activity, |this, _, cx| this.sync(cx)),
            cx.subscribe(&popover, |this, _, PopoverEvent::Dismiss, cx| {
                this.queue_open = false;
                cx.notify();
            }),
        ];
        let mut this = Self {
            session,
            activity,
            popover,
            queue_open: false,
            picked: None,
            log_scroll: UniformListScrollHandle::new(),
            log_follow: LogFollow::default(),
            ticker: None,
            _subscriptions: subscriptions,
        };
        this.sync(cx);
        this
    }

    /// The picked run, or else the repo's current run.
    fn shown<'a>(&'a self, cx: &'a App) -> Option<&'a Run> {
        self.picked.as_ref().or(self.activity.read(cx).current())
    }

    pub fn follows_current(&self) -> bool {
        self.picked.is_none()
    }

    fn show_run(&mut self, id: RunId, cx: &mut Context<Self>) {
        let activity = self.activity.read(cx);
        if activity.current().is_some_and(|run| run.id == id) {
            return self.follow_current(cx);
        }
        if let Some(run) = activity.load(id) {
            self.picked = Some(run);
            cx.notify();
        }
    }

    pub fn follow_current(&mut self, cx: &mut Context<Self>) {
        self.picked = None;
        cx.notify();
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        if let Some(picked) = &self.picked
            && !picked.state.is_finished()
            && let Some(fresh) = self.activity.read(cx).run(picked.id)
        {
            self.picked = Some(fresh.clone());
        }
        let running = self.shown(cx).is_some_and(|run| !run.state.is_finished());
        if !running {
            self.ticker = None;
        } else if self.ticker.is_none() {
            self.ticker = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        }
        cx.notify();
    }

    fn bar(&self, run: Option<&Run>, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let session = self.session.read(cx);
        let state = run.map(|run| run.state);
        let queued = self.activity.read(cx).queued().len();
        let (title, details) = match run {
            Some(run) => (run.kind.activity().title().to_string(), run_details(session, run)),
            None => ("Claude".to_string(), "No runs yet".to_string()),
        };
        let can_run_again =
            run.is_some_and(|run| run.state.is_finished()) && session.can_run_again(cx) && self.follows_current();
        let run_id = run.map(|run| run.id);
        h_flex()
            .h(px(BAR_HEIGHT))
            .flex_none()
            .px(px(8.))
            .gap(px(6.))
            .border_b_1()
            .border_color(theme.ui.line)
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
            .text_color(theme.ui.muted)
            .child(
                tag(
                    Button::new("claude-area-toggle")
                        .look(Look::Ghost)
                        .icon(Glyph::PanelBottom, 15.)
                        .w(px(24.))
                        .px(px(0.))
                        .tooltip("Hide Claude activity (⇧⌘Y)"),
                    || "claude-area:toggle",
                )
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ClaudeAreaEvent::Close))),
            )
            .child(glyph_icon(Glyph::Sparkles, 13., theme.ui.claude))
            .child(
                tag(div(), || format!("claude-area:title:{title}"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.ui.text)
                    .child(title.clone()),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(details),
            )
            .child(div().flex_1())
            .when(queued > 0, |bar| {
                bar.child(tag(div(), || format!("claude-area:queued:{queued}")).child(format!("{queued} queued")))
            })
            .when(state == Some(RunState::Running), |bar| {
                bar.child(self.queue_button(cx))
                    .child(run_button(run_id, RunAction::Pause, cx))
                    .child(stop_button(run_id, theme, cx))
            })
            .when(state == Some(RunState::Queued), |bar| {
                bar.child(stop_button(run_id, theme, cx))
            })
            .when(state == Some(RunState::Paused), |bar| {
                bar.child(run_button(run_id, RunAction::Continue, cx))
                    .child(stop_button(run_id, theme, cx))
            })
            .when(can_run_again, |bar| {
                bar.child(
                    tag(
                        Button::new("claude-area-again")
                            .icon(Glyph::RefreshCw, 12.)
                            .label("Run Again"),
                        || "claude-area:run-again",
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.update(cx, |session, cx| session.run_again(cx));
                    })),
                )
            })
            .child(self.past_runs_button(cx))
            .child(
                tag(
                    Button::new("claude-area-close")
                        .look(Look::Ghost)
                        .icon(Glyph::X, 13.)
                        .w(px(22.))
                        .px(px(0.))
                        .tooltip("Close"),
                    || "claude-area:close",
                )
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ClaudeAreaEvent::Close))),
            )
            .into_any_element()
    }

    fn queue_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let queue = self.popover.clone();
        popover("claude-area-queue")
            .anchor(Corner::BottomRight)
            .open(self.queue_open)
            .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                this.queue_open = *open;
                cx.notify();
            }))
            .trigger(tag(
                Button::new("claude-area-queue-button")
                    .icon(Glyph::ListPlus, 12.)
                    .label("Queue…"),
                || "claude-area:queue",
            ))
            .content(move |_, _, _| queue.clone())
            .into_any_element()
    }

    fn past_runs_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let area = cx.entity();
        let trigger = tag(
            Button::new("claude-area-past")
                .icon(Glyph::Clock, 12.)
                .label("Past Runs"),
            || "claude-area:past-runs",
        );
        menu_button("claude-area-past-menu", trigger, move |_, cx| past_run_rows(&area, cx))
            .anchor(Corner::BottomRight)
            .min_width(320.)
            .into_any_element()
    }

    fn plan_pane(&self, run: Option<&Run>, theme: Theme) -> AnyElement {
        let heading = |text: String| {
            div()
                .mt(px(12.))
                .mb(px(6.))
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.ui.muted)
                .child(text)
        };
        let steps = run.map(|run| run.steps.as_slice()).unwrap_or_default();
        let comments = run.map(Run::comments).unwrap_or_default();
        let writes_comments = run.is_none_or(|run| !run.kind.activity().writes_guide());
        let running = run.is_some_and(|run| !run.state.is_finished());
        v_flex()
            .id("claude-area-plan")
            .w(px(LEFT_WIDTH))
            .flex_none()
            .h_full()
            .px(px(14.))
            .py(px(10.))
            .gap(px(6.))
            .overflow_y_scroll()
            .border_r_1()
            .border_color(theme.ui.line)
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
            .text_color(theme.ui.text)
            .child(heading("Plan".into()).mt(px(0.)))
            .when(steps.is_empty(), |pane| {
                pane.child(div().text_color(theme.ui.muted).child(if running {
                    "Claude has not made a plan yet."
                } else {
                    "This run made no plan."
                }))
            })
            .children(steps.iter().map(|step| {
                let (glyph, color, bold) = match step.state {
                    StepState::Done => (Glyph::CircleCheck, theme.ui.green, false),
                    StepState::Active => (Glyph::Sparkles, theme.ui.claude, true),
                    StepState::Pending => (Glyph::CircleCheck, theme.ui.faint, false),
                };
                let name = match step.state {
                    StepState::Done => "done",
                    StepState::Active => "active",
                    StepState::Pending => "pending",
                };
                tag(h_flex().gap(px(8.)).items_start(), || {
                    format!("claude-area:step:{name}:{}", step.title)
                })
                .child(glyph_icon(glyph, 13., color).mt(px(2.)))
                .child(
                    div()
                        .min_w_0()
                        .when(bold, |text| text.font_weight(FontWeight::SEMIBOLD))
                        .child(step.title.clone()),
                )
            }))
            .when(writes_comments, |pane| {
                pane.child(heading(format!(
                    "{} · {}",
                    if running { "Comments so far" } else { "Comments" },
                    comments.len()
                )))
            })
            .children(comments.into_iter().filter(|_| writes_comments).map(|(text, at_ms)| {
                tag(h_flex().gap(px(8.)).h(px(ROW_HEIGHT)).items_center(), || {
                    format!("claude-area:comment:{text}")
                })
                .text_color(theme.ui.claude)
                .child(glyph_icon(Glyph::MessageSquare, 13., theme.ui.claude))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(text),
                )
                .children(at_ms.map(|at| {
                    div()
                        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                        .text_color(theme.ui.muted)
                        .child(mmss(at))
                }))
            }))
            .into_any_element()
    }

    fn log_pane(&mut self, run: Option<&Run>, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let running = run.is_some_and(|run| run.state == RunState::Running);
        let rows = run.map_or(0, |run| run.log.len() + usize::from(running));
        let at_bottom = {
            let state = self.log_scroll.0.borrow();
            state.last_item_size.is_none()
                || -state.base_handle.offset().y + px(1.) >= state.base_handle.max_offset().height
        };
        let scroll_to_last = self.log_follow.update(run.map(|run| run.id), rows, at_bottom);
        let following = running && self.follows_current() && self.log_follow.following;
        if following && scroll_to_last {
            self.log_scroll.scroll_to_item(rows - 1, ScrollStrategy::Bottom);
        }
        let list = uniform_list(
            "claude-area-log",
            rows,
            cx.processor(move |this: &mut Self, range: Range<usize>, _, cx| this.log_rows(range, cx)),
        )
        .track_scroll(self.log_scroll.clone())
        .flex_1()
        .min_h_0()
        .py(px(4.));
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(list)
            .child(
                h_flex()
                    .h(px(FOOTER_HEIGHT))
                    .flex_none()
                    .px(px(10.))
                    .justify_end()
                    .border_t_1()
                    .border_color(theme.ui.line)
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                    .text_color(theme.ui.muted)
                    .when(following, |footer| footer.child("Following the run")),
            )
            .into_any_element()
    }

    fn log_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = Theme::of(cx);
        let mono = cx.theme().mono_font_family.clone();
        let Some(run) = self.shown(cx) else {
            return Vec::new();
        };
        let running = run.state == RunState::Running;
        let elapsed = run.elapsed_ms(now_ms()).unwrap_or(0);
        range
            .filter_map(|index| match run.log.get(index) {
                Some(entry) => Some(log_row(entry, theme, &mono)),
                None if running => Some(thinking_row(elapsed, theme, &mono)),
                None => None,
            })
            .collect()
    }
}

impl Render for ClaudeArea {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let run = self.shown(cx).cloned();
        let bar = self.bar(run.as_ref(), theme, cx);
        let body = match &run {
            Some(_) => h_flex()
                .flex_1()
                .min_h_0()
                .child(self.plan_pane(run.as_ref(), theme))
                .child(self.log_pane(run.as_ref(), theme, cx))
                .into_any_element(),
            None => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                .text_color(theme.ui.muted)
                .child(tag(div(), || "claude-area:empty").child("Claude has not run on this review yet."))
                .child("Start a review with Ask Claude, and its plan and log show up here.")
                .into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .border_t_1()
            .border_color(theme.ui.line2)
            .child(bar)
            .child(body)
    }
}

#[derive(Clone, Copy)]
enum RunAction {
    Pause,
    Continue,
}

fn run_button(run: Option<RunId>, action: RunAction, cx: &mut Context<ClaudeArea>) -> AnyElement {
    let (id, glyph, label, name) = match action {
        RunAction::Pause => ("claude-area-pause", Glyph::Pause, "Pause", "claude-area:pause"),
        RunAction::Continue => ("claude-area-continue", Glyph::Play, "Continue", "claude-area:continue"),
    };
    tag(Button::new(id).icon(glyph, 12.).label(label), move || name)
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(id) = run {
                this.activity.update(cx, |activity, cx| match action {
                    RunAction::Pause => activity.pause(id, cx),
                    RunAction::Continue => activity.resume(id, cx),
                });
            }
        }))
        .into_any_element()
}

fn stop_button(run: Option<RunId>, theme: Theme, cx: &mut Context<ClaudeArea>) -> AnyElement {
    tag(
        Button::new("claude-area-stop")
            .child(div().size(px(9.)).rounded(px(2.)).bg(theme.danger))
            .child("Stop")
            .text_color(theme.danger),
        || "claude-area:stop",
    )
    .on_click(cx.listener(move |this, _, _, cx| {
        if let Some(id) = run {
            this.activity.update(cx, |activity, cx| activity.stop(id, cx));
        }
    }))
    .into_any_element()
}

fn run_details(session: &ReviewSession, run: &Run) -> String {
    let branch = session.host().branch_name.clone().unwrap_or_else(|| "no branch".into());
    let elapsed = match run.state {
        RunState::Queued => "waiting".to_string(),
        RunState::Paused => "paused".to_string(),
        _ => mmss(run.elapsed_ms(now_ms()).unwrap_or(0)),
    };
    format!("{branch} · {} · {elapsed}", run.model.title())
}

fn past_run_rows(area: &Entity<ClaudeArea>, cx: &mut App) -> Vec<MenuSection> {
    let theme = Theme::of(cx);
    let (runs, shown, follows) = {
        let this = area.read(cx);
        let key = this.session.read(cx).session_key();
        (
            this.activity.read(cx).past_runs(&key, PAST_RUNS),
            this.shown(cx).map(|run| run.id),
            this.follows_current(),
        )
    };
    let now = now_ms();
    let mut rows: Vec<_> = runs
        .into_iter()
        .map(|summary| {
            let area = area.clone();
            let when = summary.started_at_ms.map_or_else(|| "queued".into(), |at| ago(now, at));
            let (glyph, color) = state_glyph(summary.state, &theme);
            MenuItem::new(summary.title.clone(), move |_, cx| {
                area.update(cx, |area, cx| area.show_run(summary.id, cx));
            })
            .detail(format!(
                "{} · {} · {when}",
                summary.kind.activity().title(),
                summary.state.as_str()
            ))
            .icon(glyph, Some(color))
            .checked(shown == Some(summary.id))
            .into()
        })
        .collect();
    if rows.is_empty() {
        rows.push(MenuItem::new("No past runs", |_, _| {}).into());
    }
    let mut sections = vec![MenuSection::new(rows)];
    if !follows {
        let area = area.clone();
        sections.insert(
            0,
            MenuSection::new(vec![
                MenuItem::new("Follow current run", move |_, cx| {
                    area.update(cx, |area, cx| area.follow_current(cx));
                })
                .into(),
            ]),
        );
    }
    sections
}

pub fn state_glyph(state: RunState, theme: &Theme) -> (Glyph, Hsla) {
    match state {
        RunState::Done => (Glyph::CircleCheck, theme.ui.green),
        RunState::Failed => (Glyph::CircleAlert, theme.danger),
        RunState::Stopped => (Glyph::CircleAlert, theme.ui.muted),
        RunState::Paused => (Glyph::Pause, theme.ui.muted),
        RunState::Queued | RunState::Running => (Glyph::Spinner, theme.ui.muted),
    }
}

fn kind_name(kind: LogKind) -> &'static str {
    match kind {
        LogKind::Thinking => "thinking",
        LogKind::Message => "message",
        LogKind::Read => "read",
        LogKind::Search => "search",
        LogKind::Edit => "edit",
        LogKind::Command => "command",
        LogKind::Comment => "comment",
        LogKind::Plan => "plan",
        LogKind::Tool => "tool",
        LogKind::Blocked => "blocked",
        LogKind::Error => "error",
    }
}

fn kind_glyph(kind: LogKind) -> Glyph {
    match kind {
        LogKind::Thinking => Glyph::Clock,
        LogKind::Message => Glyph::Sparkles,
        LogKind::Read => Glyph::FileText,
        LogKind::Search => Glyph::Search,
        LogKind::Edit => Glyph::Pencil,
        LogKind::Command => Glyph::SquareTerminal,
        LogKind::Comment => Glyph::MessageSquare,
        LogKind::Plan => Glyph::List,
        LogKind::Tool => Glyph::Braces,
        LogKind::Blocked => Glyph::TriangleAlert,
        LogKind::Error => Glyph::CircleAlert,
    }
}

fn log_row(entry: &LogEntry, theme: Theme, mono: &SharedString) -> AnyElement {
    let color = match entry.kind {
        LogKind::Comment => theme.ui.claude,
        LogKind::Thinking => theme.ui.muted,
        LogKind::Blocked => theme.amber,
        LogKind::Error => theme.danger,
        _ if entry.failed => theme.danger,
        _ => theme.ui.text,
    };
    let text = first_line(&entry.text);
    tag(
        h_flex().h(px(ROW_HEIGHT)).px(px(14.)).gap(px(8.)).items_center(),
        || format!("claude-area:log:{}:{text}", kind_name(entry.kind)),
    )
    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
    .text_color(color)
    .child(time_cell(entry.at_ms, theme, mono))
    .child(glyph_icon(kind_glyph(entry.kind), 13., color))
    .child(
        div()
            .min_w_0()
            .flex_1()
            .overflow_hidden()
            .text_ellipsis()
            .whitespace_nowrap()
            .child(text),
    )
    .into_any_element()
}

fn thinking_row(elapsed_ms: u64, theme: Theme, mono: &SharedString) -> AnyElement {
    tag(
        h_flex().h(px(ROW_HEIGHT)).px(px(14.)).gap(px(8.)).items_center(),
        || "claude-area:thinking",
    )
    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
    .text_color(theme.ui.muted)
    .child(time_cell(elapsed_ms, theme, mono))
    .child(spinner("claude-area-thinking", 13., theme.ui.muted))
    .child("Thinking…")
    .into_any_element()
}

fn time_cell(at_ms: u64, theme: Theme, mono: &SharedString) -> impl IntoElement {
    div()
        .w(px(38.))
        .flex_none()
        .font_family(mono.clone())
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
        .text_color(theme.ui.faint)
        .child(mmss(at_ms))
}

pub fn mmss(ms: u64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub fn ago(now_ms: i64, then_ms: i64) -> String {
    let seconds = u64::try_from(now_ms - then_ms).unwrap_or(0) / 1000;
    let (value, unit) = match seconds {
        0..=44 => return "just now".into(),
        45..=3599 => ((seconds / 60).max(1), "min"),
        3600..=86_399 => (seconds / 3600, "hr"),
        _ => (seconds / 86_400, "day"),
    };
    let plural = if unit == "day" && value != 1 { "s" } else { "" };
    format!("{value} {unit}{plural} ago")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_follows_new_rows_until_the_user_scrolls_up() {
        let run = Some(RunId(1));
        let mut follow = LogFollow::default();
        assert!(follow.update(run, 3, true), "a new run starts at the bottom");
        assert!(!follow.update(run, 3, true), "no new row, no scroll");
        assert!(follow.update(run, 4, true), "a new row at the bottom scrolls");
        assert!(
            !follow.update(run, 5, false),
            "after a scroll up a new row does not pull the user down"
        );
        assert!(!follow.update(run, 6, false));
        assert!(follow.update(run, 7, true), "back at the bottom it follows again");
        assert!(
            follow.update(Some(RunId(2)), 2, false),
            "another run is followed from its start"
        );
    }

    #[test]
    fn a_log_time_shows_minutes_and_seconds() {
        assert_eq!(mmss(0), "0:00");
        assert_eq!(mmss(41_900), "0:41");
        assert_eq!(mmss(112_000), "1:52");
    }

    #[test]
    fn a_run_says_how_long_ago_it_started() {
        let at = |seconds: i64| ago(10_000_000, 10_000_000 - seconds * 1000);
        assert_eq!(at(10), "just now");
        assert_eq!(at(180), "3 min ago");
        assert_eq!(at(7300), "2 hr ago");
        assert_eq!(at(86_400), "1 day ago");
        assert_eq!(at(3 * 86_400), "3 days ago");
    }
}
