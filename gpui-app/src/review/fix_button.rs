use crate::review::fix_picker::{FixPicker, PickerEvent};
use crate::review::pill::RESULT_SECONDS;
use crate::review::session::{ReviewEvent, ReviewSession};
use diffpilot_core::run::{RunId, RunState};
use diffpilot_ui_kit::components::popover;
use diffpilot_ui_kit::components::{Button, Look, filled_check, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::{
    AnyElement, AppContext, Context, Corner, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Subscription, Task, Window, div, px,
};
use gpui_component::{ActiveTheme, h_flex};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixEvent {
    /// The button wants more toolbar width while its result shows.
    NeedsRoomChanged(bool),
}

pub struct FixButton {
    session: Entity<ReviewSession>,
    picker: Entity<FixPicker>,
    open: bool,
    result_for: Option<RunId>,
    needs_room: bool,
    timer: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FixEvent> for FixButton {}

impl FixButton {
    pub fn new(session: Entity<ReviewSession>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let picker = cx.new(|cx| FixPicker::new(session.clone(), cx));
        let subscriptions = vec![
            cx.observe(&session, |this, _, cx| {
                if this.open && this.session.read(cx).fixable(cx).is_empty() {
                    this.set_open(false, cx);
                }
                cx.notify();
            }),
            cx.subscribe(&session, |this, _, event, cx| {
                if matches!(event, ReviewEvent::RunChanged) {
                    this.run_changed(cx);
                }
            }),
            cx.subscribe(&picker, |this, _, PickerEvent::Dismiss, cx| this.set_open(false, cx)),
        ];
        let mut button = Self {
            session,
            picker,
            open: false,
            result_for: None,
            needs_room: false,
            timer: None,
            _subscriptions: subscriptions,
        };
        button.run_changed(cx);
        button
    }

    pub fn needs_room(&self) -> bool {
        self.needs_room
    }

    pub fn shows_result(&self) -> bool {
        self.result_for.is_some()
    }

    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        if !open {
            self.picker.update(cx, |picker, cx| picker.reset(cx));
        }
        cx.notify();
    }

    fn run_changed(&mut self, cx: &mut Context<Self>) {
        let done = self
            .session
            .read(cx)
            .fix_run(cx)
            .filter(|run| run.state == RunState::Done)
            .map(|run| run.id);
        if done.is_none() {
            self.result_for = None;
            self.timer = None;
        } else if self.result_for != done {
            self.result_for = done;
            self.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(RESULT_SECONDS))
                    .await;
                this.update(cx, |button, cx| button.hide_result(cx)).ok();
            }));
        }
        self.sync_room(cx);
        cx.notify();
    }

    fn hide_result(&mut self, cx: &mut Context<Self>) {
        self.result_for = None;
        self.timer = None;
        self.sync_room(cx);
        cx.notify();
    }

    fn sync_room(&mut self, cx: &mut Context<Self>) {
        let needs = self.shows_result();
        if needs != self.needs_room {
            self.needs_room = needs;
            cx.emit(FixEvent::NeedsRoomChanged(needs));
        }
    }

    fn content(&self, theme: Theme, cx: &Context<Self>) -> Option<AnyElement> {
        let session = self.session.read(cx);
        let button = |id: &'static str| {
            Button::new(id)
                .look(Look::Ghost)
                .regular()
                .rounded_full()
                .h(px(30.))
                .gap(px(6.))
        };
        if let Some(problem) = session.fix_problem(cx) {
            return Some(
                tag(button("fix-failed"), || "fix:failed")
                    .icon_color(Glyph::TriangleAlert, 16., theme.amber)
                    .label("Fix failed")
                    .tooltip(problem)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.update(cx, |session, cx| session.dismiss_fix_problem(cx));
                    }))
                    .into_any_element(),
            );
        }
        if self.shows_result()
            && let Some((fixed, open)) = session.fix_outcome(cx)
        {
            let text = if open == 0 {
                format!("Fixed {fixed}")
            } else {
                format!("Fixed {fixed} · {open} open")
            };
            return Some(
                tag(button("fix-result"), || "fix:result")
                    .child(filled_check(theme.ui.green, 16.))
                    .child(div().child(text))
                    .into_any_element(),
            );
        }
        let count = session.fixable(cx).len();
        if count == 0 {
            return None;
        }
        let trigger = tag(
            Button::new("fix-open")
                .look(Look::Claude)
                .regular()
                .rounded_full()
                .h(px(30.))
                .gap(px(6.))
                .icon(Glyph::Pencil, 14.)
                .label(format!("Fix {count}"))
                .tooltip("Pick the comments Claude should fix"),
            || "fix:open",
        );
        Some(
            tag(div(), || format!("fix:ready:{count}"))
                .child(
                    popover("fix-popover")
                        .anchor(Corner::TopRight)
                        .open(self.open)
                        .on_open_change(cx.listener(|this, open: &bool, _, cx| this.set_open(*open, cx)))
                        .trigger(trigger)
                        .content({
                            let picker = self.picker.clone();
                            move |_, _, _| picker.clone()
                        }),
                )
                .into_any_element(),
        )
    }
}

impl Render for FixButton {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let content = self.content(theme, cx);
        h_flex()
            .items_center()
            .text_size(diffpilot_ui_kit::text_px(13.))
            .text_color(cx.theme().foreground)
            .children(content.map(|content| div().ml_1p5().child(content)))
    }
}
