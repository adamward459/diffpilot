use diffpilot_core::commit::{self, CommitMessage};
use diffpilot_ui_kit::components::{Button, Look, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Icon, h_flex, v_flex};
use std::path::PathBuf;

pub const WIDTH: f32 = 400.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitEvent {
    Dismiss,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommitState {
    Idle,
    Committing,
    Failed(SharedString),
}

pub struct CommitPopover {
    repo: PathBuf,
    changed: usize,
    message: Entity<InputState>,
    state: CommitState,
    _subscription: Subscription,
}

impl EventEmitter<CommitEvent> for CommitPopover {}

impl CommitPopover {
    pub fn new(repo: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| InputState::new(window, cx).auto_grow(3, 8));
        let subscription = cx.subscribe_in(
            &message,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { secondary: true } => this.commit(window, cx),
                _ => {}
            },
        );
        Self {
            repo,
            changed: 0,
            message,
            state: CommitState::Idle,
            _subscription: subscription,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.message.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn set_changed(&mut self, changed: usize, cx: &mut Context<Self>) {
        if self.changed != changed {
            self.changed = changed;
            cx.notify();
        }
    }

    fn parsed(&self, cx: &Context<Self>) -> Option<CommitMessage> {
        CommitMessage::parse(self.message.read(cx).value().as_ref())
    }

    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(message) = self.parsed(cx) else { return };
        if self.state == CommitState::Committing {
            return;
        }
        self.state = CommitState::Committing;
        cx.notify();
        let repo = self.repo.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { commit::commit_all(&repo, &message) })
                .await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.state = CommitState::Idle;
                    this.message.update(cx, |input, cx| input.set_value("", window, cx));
                    cx.emit(CommitEvent::Dismiss);
                    cx.notify();
                }
                Err(error) => {
                    this.state = CommitState::Failed(error.to_string().into());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

impl Render for CommitPopover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let busy = self.state == CommitState::Committing;
        let ready = self.parsed(cx).is_some() && !busy;
        let plural = if self.changed == 1 { "file" } else { "files" };
        v_flex()
            .w(px(WIDTH - 24.))
            .p(px(4.))
            .text_size(px(13.))
            .text_color(theme.ui.text)
            .child(
                h_flex()
                    .gap_2()
                    .mb(px(4.))
                    .items_center()
                    .child(Icon::new(Glyph::GitCommit).size(px(13.)).text_color(theme.ui.muted))
                    .child(div().font_weight(FontWeight::BOLD).child("Commit changes")),
            )
            .child(
                div()
                    .mb(px(12.))
                    .text_size(px(12.))
                    .text_color(theme.ui.muted)
                    .child(format!(
                        "Stages all {} changed {plural} and commits them.",
                        self.changed
                    )),
            )
            .child(
                div()
                    .mb(px(12.))
                    .child(Input::new(&self.message).text_size(px(13.)))
                    .when(self.message.read(cx).value().is_empty(), |field| {
                        field.child(
                            div()
                                .absolute()
                                .top(px(9.))
                                .left(px(13.))
                                .text_size(px(13.))
                                .text_color(theme.ui.muted)
                                .child("Commit message"),
                        )
                    })
                    .relative(),
            )
            .when_some(
                match &self.state {
                    CommitState::Failed(text) => Some(text.clone()),
                    _ => None,
                },
                |col, text| {
                    col.child(
                        div()
                            .mb(px(12.))
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(theme.danger)
                            .child(text),
                    )
                },
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .justify_end()
                    .child(
                        Button::new("cancel-commit")
                            .label("Cancel")
                            .regular()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(CommitEvent::Dismiss))),
                    )
                    .child(
                        tag(
                            Button::new("commit-submit")
                                .label(if busy { "Committing\u{2026}" } else { "Commit" })
                                .hint("⌘↩")
                                .regular()
                                .look(Look::Primary)
                                .disabled(!ready),
                            || "commit:submit",
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                    ),
            )
    }
}
