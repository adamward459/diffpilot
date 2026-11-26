use crate::review::popover::{PopoverEvent, ReviewPopover};
use crate::review::session::{ReviewEvent, ReviewSession};
use diffpilot_core::review::{ReviewRun, Severity};
use diffpilot_core::run::{Progress, RunState};
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
use std::time::{Duration, Instant};

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
}

pub struct ReviewPill {
    session: Entity<ReviewSession>,
    popover: Entity<ReviewPopover>,
    open: bool,
    shows_result: bool,
    shows_fix_result: bool,
    needs_room: bool,
    ticker: Option<Task<()>>,
    result_timer: Option<Task<()>>,
    fix_ticker: Option<Task<()>>,
    fix_timer: Option<Task<()>>,
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
                ReviewEvent::FixChanged => this.fix_changed(cx),
                ReviewEvent::ThreadsChanged(_) => {}
            }),
            cx.subscribe(&popover, |this, _, PopoverEvent::Dismiss, cx| this.set_open(false, cx)),
        ];
        let mut pill = Self {
            session,
            popover,
            open: false,
            shows_result: false,
            shows_fix_result: false,
            needs_room: false,
            ticker: None,
            result_timer: None,
            fix_ticker: None,
            fix_timer: None,
            _subscriptions: subscriptions,
        };
        pill.run_changed(cx);
        pill.fix_changed(cx);
        pill
    }

    pub fn needs_room(&self) -> bool {
        self.needs_room
    }

    pub fn is_running(&self, cx: &App) -> bool {
        matches!(self.session.read(cx).run(), ReviewRun::Running { .. })
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
        self.shows_result
    }

    pub fn shows_fix_result(&self) -> bool {
        self.shows_fix_result
    }

    fn fix_changed(&mut self, cx: &mut Context<Self>) {
        let (fixing, done) = {
            let session = self.session.read(cx);
            let done = session.fix_run().is_some_and(|run| run.state == RunState::Done);
            (session.fixing(), done)
        };
        if !fixing {
            self.fix_ticker = None;
        } else if self.fix_ticker.is_none() {
            self.fix_ticker = Some(Self::tick_every_second(Instant::now(), cx));
        }
        if !done {
            self.shows_fix_result = false;
            self.fix_timer = None;
        } else if !self.shows_fix_result {
            self.shows_fix_result = true;
            self.fix_timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(RESULT_SECONDS))
                    .await;
                this.update(cx, |pill, cx| pill.hide_fix_result(cx)).ok();
            }));
        }
        self.sync_room(cx);
        cx.notify();
    }

    fn hide_fix_result(&mut self, cx: &mut Context<Self>) {
        self.shows_fix_result = false;
        self.fix_timer = None;
        self.sync_room(cx);
        cx.notify();
    }

    fn run_changed(&mut self, cx: &mut Context<Self>) {
        let run = self.session.read(cx).run().clone();
        self.shows_result = matches!(run, ReviewRun::Done { .. });
        self.ticker = None;
        self.result_timer = None;
        match run {
            ReviewRun::Running { since } => self.ticker = Some(Self::tick_every_second(since, cx)),
            ReviewRun::Failed(_) => self.open = true,
            ReviewRun::Done { .. } => {
                self.result_timer = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_secs(RESULT_SECONDS))
                        .await;
                    this.update(cx, |pill, cx| pill.hide_result(cx)).ok();
                }));
            }
            ReviewRun::Idle => {}
        }
        self.sync_room(cx);
        cx.notify();
    }

    fn hide_result(&mut self, cx: &mut Context<Self>) {
        self.shows_result = false;
        self.result_timer = None;
        self.sync_room(cx);
        cx.notify();
    }

    fn sync_room(&mut self, cx: &mut Context<Self>) {
        let session = self.session.read(cx);
        let running = matches!(session.run(), ReviewRun::Running { .. }) || session.fixing();
        let needs = running || self.shows_result || self.shows_fix_result;
        if needs != self.needs_room {
            self.needs_room = needs;
            cx.emit(PillEvent::NeedsRoomChanged(needs));
        }
    }

    fn tick_every_second(since: Instant, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                let into_second = since.elapsed().subsec_nanos();
                let wait = Duration::from_nanos(1_000_000_000 - u64::from(into_second));
                cx.background_executor().timer(wait).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
    }

    fn trigger(&self, run: &ReviewRun, visible: bool, theme: Theme) -> Button {
        let button = tag(
            Button::new("claude-pill")
                .look(Look::Ghost)
                .regular()
                .rounded_full()
                .h(px(30.))
                .gap(px(6.)),
            || "review:open",
        );
        match run {
            ReviewRun::Done { counts } if self.shows_result => button
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
                })),
            ReviewRun::Failed(_) => button
                .icon_color(Glyph::TriangleAlert, 16., theme.amber)
                .label("Review failed"),
            _ => button
                .icon_color(Glyph::Sparkles, 16., theme.ui.claude)
                .label("Ask Claude")
                .disabled(!visible),
        }
    }

    fn running(&self, since: Instant, theme: Theme, cx: &Context<Self>) -> impl IntoElement {
        working(
            Working {
                label: "Reviewing".into(),
                activity: None,
                seconds: since.elapsed().as_secs(),
                stop_id: "stop-review",
                stop: Self::stop,
            },
            theme,
            cx,
        )
    }

    fn fixing(&self, session: &ReviewSession, theme: Theme, cx: &Context<Self>) -> Option<AnyElement> {
        let run = session.fix_run().filter(|run| !run.state.is_finished())?;
        let label = match run.progress() {
            Some(Progress { done, total }) if run.state == RunState::Running => format!("Fixing {done} of {total}"),
            _ if run.state == RunState::Queued => "Waiting".to_string(),
            _ => "Fixing".to_string(),
        };
        let seconds = run.started_at_ms.map_or(0, |started| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis() as i64);
            u64::try_from(now - started).unwrap_or(0) / 1000
        });
        let activity = run.activity.clone().map(SharedString::from);
        Some(
            working(
                Working {
                    label,
                    activity,
                    seconds,
                    stop_id: "stop-fix",
                    stop: Self::stop_fix,
                },
                theme,
                cx,
            )
            .into_any_element(),
        )
    }

    fn fix_area(&self, session: &ReviewSession, theme: Theme, cx: &Context<Self>) -> Option<AnyElement> {
        if let Some(working) = self.fixing(session, theme, cx) {
            return Some(working);
        }
        let button = |id: &'static str| {
            Button::new(id)
                .look(Look::Ghost)
                .regular()
                .rounded_full()
                .h(px(30.))
                .gap(px(6.))
        };
        if let Some(problem) = session.fix_problem() {
            return Some(
                button("fix-failed")
                    .icon_color(Glyph::TriangleAlert, 16., theme.amber)
                    .label("Fix failed")
                    .tooltip(problem)
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss_fix_problem(cx)))
                    .into_any_element(),
            );
        }
        if self.shows_fix_result
            && let Some((fixed, open)) = session.fix_outcome()
        {
            let text = if open == 0 {
                format!("Fixed {fixed}")
            } else {
                format!("Fixed {fixed} · {open} open")
            };
            return Some(
                tag(button("fix-result"), || "pill:fix-result")
                    .child(filled_check(theme.ui.green, 16.))
                    .child(div().child(text))
                    .into_any_element(),
            );
        }
        let count = session.fixable().len();
        if count == 0 {
            return None;
        }
        let blocker = session.fix_blocker(cx);
        let tooltip = blocker
            .clone()
            .unwrap_or_else(|| "Ask Claude to fix the open comments".into());
        Some(
            tag(div(), || format!("pill:fix-ready:{count}"))
                .child(
                    tag(
                        Button::new("fix-threads")
                            .look(Look::Claude)
                            .regular()
                            .rounded_full()
                            .h(px(30.))
                            .gap(px(6.))
                            .icon(Glyph::Pencil, 14.)
                            .label(format!("Fix {count}"))
                            .tooltip(tooltip)
                            .disabled(blocker.is_some()),
                        || "pill:fix-threads",
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.fix(cx))),
                )
                .into_any_element(),
        )
    }

    pub fn fix(&mut self, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| session.fix_threads(&[], cx));
    }

    pub fn stop_fix(&mut self, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| session.stop_fix(cx));
    }

    pub fn dismiss_fix_problem(&mut self, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| session.dismiss_fix_problem(cx));
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| session.stop_review(cx));
    }
}

impl Render for ReviewPill {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let session = self.session.read(cx);
        let run = session.run().clone();
        let visible = session.host().visible_path.is_some();
        let fix = self.fix_area(session, theme, cx);
        let claude = match &run {
            ReviewRun::Running { since } => self.running(*since, theme, cx).into_any_element(),
            _ => Popover::new("review-popover")
                .anchor(Corner::TopRight)
                .open(self.open)
                .on_open_change(cx.listener(|this, open: &bool, _, cx| this.set_open(*open, cx)))
                .trigger(self.trigger(&run, visible, theme))
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
            .children(fix)
    }
}

struct Working {
    label: String,
    activity: Option<SharedString>,
    seconds: u64,
    stop_id: &'static str,
    stop: fn(&mut ReviewPill, &mut Context<ReviewPill>),
}

fn working(run: Working, theme: Theme, cx: &Context<ReviewPill>) -> impl IntoElement {
    let Working {
        label,
        activity,
        seconds,
        stop_id,
        stop,
    } = run;
    h_flex()
        .gap(px(7.))
        .pl_2p5()
        .pr_1()
        .items_center()
        .child(
            Spinner::new()
                .icon(Icon::new(Glyph::Spinner))
                .color(theme.ui.muted)
                .with_size(px(16.)),
        )
        .child(
            h_flex()
                .id((stop_id, 0usize))
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
                )
                .when_some(activity, |row, text| {
                    row.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
                }),
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
            .on_click(cx.listener(move |this, _, _, cx| stop(this, cx))),
        )
}
