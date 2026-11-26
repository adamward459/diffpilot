use crate::review::popover::{PopoverEvent, ReviewPopover};
use crate::review::session::{ReviewEvent, ReviewSession};
use diffpilot_core::review::{ReviewRun, Severity};
use diffpilot_core::threads::ThreadId;
use diffpilot_ui_kit::components::{Button, Look, filled_check, severity_badge};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::{
    App, AppContext, Context, Corner, Entity, EventEmitter, FontWeight, Global, IntoElement, KeyBinding, ParentElement,
    Render, Styled, Subscription, Task, WeakEntity, Window, actions, div, px,
};
use gpui_component::popover::Popover;
use gpui_component::spinner::Spinner;
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
            target.update(cx, |session, cx| session.stop_review(cx)).ok();
        }
        cx.default_global::<StopTargets>().0 = live;
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillEvent {
    /// The pill wants more toolbar width: while a review runs and while its result shows.
    NeedsRoomChanged(bool),
    CommentOnFile(ThreadId),
}

pub struct ReviewPill {
    session: Entity<ReviewSession>,
    popover: Entity<ReviewPopover>,
    open: bool,
    shows_result: bool,
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
            cx.subscribe(&session, |this, _, event, cx| {
                if *event == ReviewEvent::RunChanged {
                    this.run_changed(cx);
                }
            }),
            cx.subscribe(&popover, |this, _, PopoverEvent::Dismiss, cx| this.set_open(false, cx)),
        ];
        let mut pill = Self {
            session,
            popover,
            open: false,
            shows_result: false,
            needs_room: false,
            ticker: None,
            result_timer: None,
            _subscriptions: subscriptions,
        };
        pill.run_changed(cx);
        pill
    }

    pub fn needs_room(&self) -> bool {
        self.needs_room
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
        let running = matches!(self.session.read(cx).run(), ReviewRun::Running { .. });
        let needs = running || self.shows_result;
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

    pub fn comment_on_file(&mut self, cx: &mut Context<Self>) {
        let created = self
            .session
            .update(cx, |session, cx| session.comment_on_visible_file(cx));
        if let Some(id) = created {
            cx.emit(PillEvent::CommentOnFile(id));
        }
    }

    fn trigger(&self, run: &ReviewRun, visible: bool, theme: Theme) -> Button {
        let button = Button::new("claude-pill")
            .look(Look::Ghost)
            .regular()
            .rounded_full()
            .h(px(24.))
            .gap(px(6.));
        match run {
            ReviewRun::Done { counts } if self.shows_result => button
                .child(filled_check(theme.ui.green, 16.))
                .child(div().child("Review done"))
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
        let seconds = since.elapsed().as_secs();
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
                    .gap_1()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Reviewing"))
                    .child(
                        div()
                            .text_color(theme.ui.muted)
                            .child(format!("{}:{:02}", seconds / 60, seconds % 60)),
                    ),
            )
            .child(
                Button::new("stop-review")
                    .look(Look::Ghost)
                    .rounded_full()
                    .w(px(22.))
                    .px(px(0.))
                    .tooltip("Stop Claude (⌘.)")
                    .child(div().size(px(10.)).rounded(px(2.)).bg(theme.danger))
                    .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
            )
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
        let count = session.thread_count();
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
            .child(div().w(px(1.)).h(px(16.)).mx_0p5().bg(theme.ui.line))
            .child(
                Button::new("comment-on-file")
                    .look(Look::Ghost)
                    .regular()
                    .rounded_full()
                    .h(px(24.))
                    .gap(px(6.))
                    .icon(Glyph::MessageSquare, 16.)
                    .label(count.to_string())
                    .tooltip("Comment on file")
                    .disabled(!visible)
                    .on_click(cx.listener(|this, _, _, cx| this.comment_on_file(cx))),
            )
    }
}
