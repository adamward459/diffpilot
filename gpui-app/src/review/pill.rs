use crate::review::popover::{PopoverEvent, ReviewPopover};
use crate::review::session::{ReviewEvent, ReviewSession};
use diffpilot_core::review::Severity;
use diffpilot_core::run::{Progress, Run, RunId, RunKind, RunState};
use diffpilot_ui_kit::components::{Button, Look, filled_check, severity_badge, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, Corner, Entity, EventEmitter, FontWeight, Global, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Task, WeakEntity, Window, actions, div, px,
};
use gpui_component::popover::Popover;
use gpui_component::spinner::Spinner;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme, Icon, Sizable, h_flex};
use std::time::Duration;

actions!(review_ui, [StopReview]);

pub const RESULT_SECONDS: u64 = 5;

/// Sessions with a pill on screen. ⌘. stops their reviews wherever the focus is.
#[derive(Default)]
struct StopTargets(Vec<WeakEntity<ReviewSession>>);

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
            target.update(cx, |session, cx| session.stop_claude(cx)).ok();
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
        cx.default_global::<StopTargets>().0.push(session.downgrade());
        let subscriptions = vec![
            cx.observe(&session, |_, _, cx| cx.notify()),
            cx.subscribe(&session, |this, _, event, cx| match event {
                ReviewEvent::RunChanged => this.run_changed(cx),
                ReviewEvent::RunFinished(RunState::Failed) => this.show_review_problem(cx),
                ReviewEvent::RunFinished(_) | ReviewEvent::ThreadsChanged(_) => {}
            }),
            cx.subscribe(&popover, |this, _, PopoverEvent::Dismiss, cx| this.set_open(false, cx)),
        ];
        let mut pill = Self {
            session,
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
        self.session.read(cx).is_busy()
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
        if self.session.read(cx).review_problem().is_some() {
            self.open = true;
            cx.notify();
        }
    }

    fn run_changed(&mut self, cx: &mut Context<Self>) {
        let (running, done_review) = {
            let current = self.session.read(cx).current();
            (
                current.is_some_and(|run| !run.state.is_finished()),
                current
                    .filter(|run| run.kind == RunKind::Review && run.state == RunState::Done)
                    .map(|run| run.id),
            )
        };
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

    fn sync_room(&mut self, cx: &mut Context<Self>) {
        let running = self
            .session
            .read(cx)
            .current()
            .is_some_and(|run| !run.state.is_finished());
        let needs = running || self.shows_result();
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

    fn trigger(&self, session: &ReviewSession, visible: bool, theme: Theme) -> Button {
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
        if session.review_problem().is_some() {
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
        let label = match (run.kind, run.progress()) {
            (_, _) if run.state == RunState::Queued => "Waiting".to_string(),
            (RunKind::Review, _) => "Reviewing".to_string(),
            (RunKind::FixComments, Some(Progress { done, total })) => format!("Fixing {done} of {total}"),
            (RunKind::FixComments, None) => "Fixing".to_string(),
            (RunKind::MakeChanges, _) => "Making changes".to_string(),
            (RunKind::Guide, _) => "Writing guide".to_string(),
            (RunKind::Reply, _) => "Replying".to_string(),
        };
        let seconds = run.elapsed_ms(crate::runs::now_ms()).unwrap_or(0) / 1000;
        let stop_id = match run.kind {
            RunKind::Review => "stop-review",
            RunKind::FixComments => "stop-fix",
            _ => "stop-run",
        };
        working(
            Working {
                label,
                activity: run.activity.clone().map(SharedString::from),
                seconds,
                stop_id,
            },
            theme,
            cx,
        )
        .into_any_element()
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| session.stop_claude(cx));
    }
}

impl Render for ReviewPill {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let session = self.session.read(cx);
        let visible = session.host().visible_path.is_some();
        let claude = match session.current().filter(|run| !run.state.is_finished()) {
            Some(run) => self.working(run, theme, cx),
            None => Popover::new("review-popover")
                .anchor(Corner::TopRight)
                .open(self.open)
                .on_open_change(cx.listener(|this, open: &bool, _, cx| {
                    if *open && this.shows_result() {
                        cx.emit(PillEvent::ShowArea);
                    } else {
                        this.set_open(*open, cx);
                    }
                }))
                .trigger(self.trigger(session, visible, theme))
                .content({
                    let popover = self.popover.clone();
                    move |_, _, _| popover.clone()
                })
                .into_any_element(),
        };
        h_flex()
            .gap_1p5()
            .items_center()
            .text_size(px(13.))
            .text_color(cx.theme().foreground)
            .child(claude)
    }
}

struct Working {
    label: String,
    activity: Option<SharedString>,
    seconds: u64,
    stop_id: &'static str,
}

fn working(run: Working, theme: Theme, cx: &Context<ReviewPill>) -> impl IntoElement {
    let Working {
        label,
        activity,
        seconds,
        stop_id,
    } = run;
    h_flex()
        .gap(px(7.))
        .pl_2p5()
        .pr_1()
        .items_center()
        .child(
            h_flex()
                .id((stop_id, 0usize))
                .gap(px(7.))
                .items_center()
                .cursor_pointer()
                .child(
                    Spinner::new()
                        .icon(Icon::new(Glyph::Spinner))
                        .color(theme.ui.muted)
                        .with_size(px(16.)),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            tag(div(), || format!("pill:{stop_id}:working"))
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
        .child(
            tag(
                Button::new(stop_id)
                    .look(Look::Ghost)
                    .rounded_full()
                    .w(px(22.))
                    .px(px(0.))
                    .tooltip("Stop Claude (⌘.)")
                    .child(div().size(px(10.)).rounded(px(2.)).bg(theme.danger)),
                || format!("pill:{stop_id}"),
            )
            .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
        )
}
