use crate::claude_activity::ClaudeActivity;
use crate::review::popover::{PopoverEvent, ReviewPopover};
use crate::review::session::{ReviewEvent, ReviewSession};
use diffpilot_core::review::Severity;
use diffpilot_core::run::{Run, RunId, RunState};
use diffpilot_ui_kit::components::{Button, Look, filled_check, severity_badge, tag};
use diffpilot_ui_kit::components::{glyph_icon, popover, spinner};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, Corner, Entity, EventEmitter, FontWeight, Global, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Task, WeakEntity, Window, actions, div, px,
};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, h_flex};
use std::time::Duration;

actions!(review_ui, [StopReview]);

pub const RESULT_SECONDS: u64 = 5;

/// Repos with a pill on screen. ⌘. stops their runs wherever the focus is.
#[derive(Default)]
struct StopTargets(Vec<WeakEntity<ClaudeActivity>>);

impl Global for StopTargets {}

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-.", StopReview, None)]);
    cx.on_action(|_: &StopReview, cx| {
        let targets = std::mem::take(&mut cx.default_global::<StopTargets>().0);
        let live: Vec<_> = targets
            .into_iter()
            .filter(|target| target.upgrade().is_some())
            .collect();
        for target in &live {
            target.update(cx, |activity, cx| activity.stop_current(cx)).ok();
        }
        cx.default_global::<StopTargets>().0 = live;
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillEvent {
    /// The pill wants more toolbar width: while a review runs and while its result shows.
    NeedsRoomChanged(bool),
    ShowArea,
}

pub struct ReviewPill {
    session: Entity<ReviewSession>,
    activity: Entity<ClaudeActivity>,
    popover: Entity<ReviewPopover>,
    open: bool,
    result_for: Option<RunId>,
    needs_room: bool,
    ticker: Option<Task<()>>,
    result_timer: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PillEvent> for ReviewPill {}

impl ReviewPill {
    pub fn new(session: Entity<ReviewSession>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let popover = cx.new(|cx| ReviewPopover::new(session.clone(), window, cx));
        let activity = session.read(cx).activity().clone();
        cx.default_global::<StopTargets>().0.push(activity.downgrade());
        let subscriptions = vec![
            cx.observe(&session, |_, _, cx| cx.notify()),
            cx.observe(&activity, |this, _, cx| this.run_changed(cx)),
            cx.subscribe(&session, |this, _, event, cx| match event {
                ReviewEvent::RunChanged => this.run_changed(cx),
                ReviewEvent::RunFinished(RunState::Failed) => this.show_review_problem(cx),
                ReviewEvent::RunFinished(_) | ReviewEvent::ThreadsChanged(_) => {}
            }),
            cx.subscribe(&popover, |this, _, PopoverEvent::Dismiss, cx| this.set_open(false, cx)),
        ];
        let mut pill = Self {
            session,
            activity,
            popover,
            open: false,
            result_for: None,
            needs_room: false,
            ticker: None,
            result_timer: None,
            _subscriptions: subscriptions,
        };
        pill.run_changed(cx);
        pill
    }

    pub fn popover(&self) -> Entity<ReviewPopover> {
        self.popover.clone()
    }

    pub fn needs_room(&self) -> bool {
        self.needs_room
    }

    pub fn is_running(&self, cx: &App) -> bool {
        self.activity.read(cx).is_busy()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            cx.notify();
        }
    }

    pub fn shows_result(&self) -> bool {
        self.result_for.is_some()
    }

    fn show_review_problem(&mut self, cx: &mut Context<Self>) {
        if self.session.read(cx).review_problem(cx).is_some() {
            self.open = true;
            cx.notify();
        }
    }

    fn run_changed(&mut self, cx: &mut Context<Self>) {
        let running = self.running(cx);
        let done_review = self
            .session
            .read(cx)
            .review_run(cx)
            .filter(|run| run.state == RunState::Done)
            .map(|run| run.id);
        if !running {
            self.ticker = None;
        } else if self.ticker.is_none() {
            self.ticker = Some(Self::tick_every_second(cx));
        }
        if done_review.is_none() {
            self.result_for = None;
            self.result_timer = None;
        } else if self.result_for != done_review {
            self.result_for = done_review;
            self.result_timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(RESULT_SECONDS))
                    .await;
                this.update(cx, |pill, cx| pill.hide_result(cx)).ok();
            }));
        }
        self.sync_room(cx);
        cx.notify();
    }

    fn hide_result(&mut self, cx: &mut Context<Self>) {
        self.result_for = None;
        self.result_timer = None;
        self.sync_room(cx);
        cx.notify();
    }

    fn running(&self, cx: &App) -> bool {
        self.activity
            .read(cx)
            .current()
            .is_some_and(|run| !run.state.is_finished())
    }

    fn sync_room(&mut self, cx: &mut Context<Self>) {
        let needs = self.running(cx) || self.shows_result();
        if needs != self.needs_room {
            self.needs_room = needs;
            cx.emit(PillEvent::NeedsRoomChanged(needs));
        }
    }

    fn tick_every_second(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
    }

    fn trigger(&self, session: &ReviewSession, visible: bool, theme: Theme, cx: &App) -> Button {
        let button = tag(
            Button::new("claude-pill")
                .look(Look::Ghost)
                .regular()
                .rounded_full()
                .h(px(30.))
                .gap(px(6.)),
            || "review:open",
        );
        if self.shows_result() {
            let counts = session.review_counts();
            return button
                .child(filled_check(theme.ui.green, 16.))
                .child(tag(div(), || "review:done").child("Review done"))
                .children(Severity::ALL.iter().filter_map(|severity| {
                    let count = counts.get(severity).copied().unwrap_or(0);
                    (count > 0).then(|| {
                        h_flex()
                            .gap(px(2.))
                            .items_center()
                            .text_color(theme.severity(*severity))
                            .child(severity_badge(*severity, &theme, 12.5))
                            .child(div().font_weight(FontWeight::MEDIUM).child(count.to_string()))
                    })
                }));
        }
        if session.review_problem(cx).is_some() {
            return button
                .icon_color(Glyph::TriangleAlert, 16., theme.amber)
                .label("Review failed");
        }
        button
            .icon_color(Glyph::Sparkles, 16., theme.ui.claude)
            .label("Ask Claude")
            .disabled(!visible)
    }

    fn working(&self, run: &Run, theme: Theme, cx: &Context<Self>) -> AnyElement {
        let activity = run.kind.activity();
        let label = match run.state {
            RunState::Queued => "Waiting".to_string(),
            RunState::Paused => "Paused".to_string(),
            _ => activity.running_label(run.progress()),
        };
        let seconds = run.elapsed_ms(crate::runs::now_ms()).unwrap_or(0) / 1000;
        working(
            Working {
                run: run.id,
                state: run.state,
                title: activity.title(),
                label,
                activity: run.activity.clone().map(SharedString::from),
                seconds,
            },
            theme,
            cx,
        )
        .into_any_element()
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.activity.update(cx, |activity, cx| activity.stop_current(cx));
    }
}

impl Render for ReviewPill {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let session = self.session.read(cx);
        let visible = session.host().visible_path.is_some();
        let working = self.activity.read(cx).current().filter(|run| !run.state.is_finished());
        let claude = match working {
            Some(run) => self.working(run, theme, cx),
            None => popover("review-popover")
                .anchor(Corner::TopRight)
                .open(self.open)
                .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                    if *open && this.shows_result() {
                        cx.emit(PillEvent::ShowArea);
                    } else {
                        this.set_open(*open, cx);
                    }
                }))
                .trigger(self.trigger(session, visible, theme, cx))
                .content({
                    let popover = self.popover.clone();
                    move |_, _, _| popover.clone()
                })
                .into_any_element(),
        };
        h_flex()
            .gap_1p5()
            .items_center()
            .text_size(diffpilot_ui_kit::text_px(13.))
            .text_color(cx.theme().foreground)
            .child(claude)
    }
}

struct Working {
    run: RunId,
    state: RunState,
    title: &'static str,
    label: String,
    activity: Option<SharedString>,
    seconds: u64,
}

fn working(run: Working, theme: Theme, cx: &Context<ReviewPill>) -> impl IntoElement {
    let Working {
        run,
        state,
        title,
        label,
        activity,
        seconds,
    } = run;
    let paused = state == RunState::Paused;
    h_flex()
        .gap(px(7.))
        .pl_2p5()
        .pr_1()
        .items_center()
        .child(
            h_flex()
                .id("pill-working")
                .gap(px(7.))
                .items_center()
                .cursor_pointer()
                .child(if paused {
                    glyph_icon(Glyph::Pause, 16., theme.ui.muted).into_any_element()
                } else {
                    spinner(("pill-spinner", 0usize), 16., theme.ui.muted).into_any_element()
                })
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            tag(div(), move || format!("pill:working:{title}"))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label),
                        )
                        .child(
                            div()
                                .text_color(theme.ui.muted)
                                .child(format!("{}:{:02}", seconds / 60, seconds % 60)),
                        ),
                )
                .when_some(activity, |row, text| {
                    row.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
                })
                .on_click(cx.listener(|_, _, _, cx| cx.emit(PillEvent::ShowArea))),
        )
        .when(state == RunState::Running, |row| {
            row.child(
                tag(
                    Button::new("pill-pause")
                        .look(Look::Ghost)
                        .rounded_full()
                        .w(px(22.))
                        .px(px(0.))
                        .tooltip("Pause Claude")
                        .icon(Glyph::Pause, 12.),
                    || "pill:pause",
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.activity.update(cx, |activity, cx| activity.pause(run, cx));
                })),
            )
        })
        .when(paused, |row| {
            row.child(
                tag(
                    Button::new("pill-continue")
                        .look(Look::Ghost)
                        .rounded_full()
                        .w(px(22.))
                        .px(px(0.))
                        .tooltip("Continue Claude")
                        .icon(Glyph::Play, 12.),
                    || "pill:continue",
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.activity.update(cx, |activity, cx| activity.resume(run, cx));
                })),
            )
        })
        .child(
            tag(
                Button::new("pill-stop")
                    .look(Look::Ghost)
                    .rounded_full()
                    .w(px(22.))
                    .px(px(0.))
                    .tooltip("Stop Claude (⌘.)")
                    .child(div().size(px(10.)).rounded(px(2.)).bg(theme.danger)),
                || "pill:stop",
            )
            .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
        )
}
