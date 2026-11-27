use crate::runs::Runs;
use diffpilot_core::commit::{self, CommitMessage};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::review::ReviewModel;
use diffpilot_core::run::{Run, RunId, RunOutcome, RunState};
use diffpilot_core::runner::RunEvent;
use diffpilot_core::store::SessionKey;
use diffpilot_ui_kit::components::{Button, Look, spinner, tag};
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
    Generating(Option<RunId>),
    Failed(SharedString),
}

pub struct CommitPopover {
    repo: PathBuf,
    changed: usize,
    message: Entity<InputState>,
    state: CommitState,
    session: SessionKey,
    turn: u64,
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
                InputEvent::Change => {
                    if matches!(this.state, CommitState::Failed(_)) {
                        this.state = CommitState::Idle;
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { secondary: true } => this.commit(window, cx),
                _ => {}
            },
        );
        let session = SessionKey::for_comparison(&repo, &Comparison::WorkingTree, None);
        Self {
            repo,
            changed: 0,
            message,
            state: CommitState::Idle,
            session,
            turn: 0,
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

    pub fn set_session(&mut self, session: SessionKey) {
        self.session = session;
    }

    fn generating(&self) -> bool {
        matches!(self.state, CommitState::Generating(_))
    }

    fn fail(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.state = CommitState::Failed(text.into());
        cx.notify();
    }

    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.generating() || self.state == CommitState::Committing {
            return;
        }
        let Some(runs) = cx.try_global::<Runs>() else {
            return self.fail("Claude runs are not available here.", cx);
        };
        if let Some(reason) = runs.unavailable() {
            let reason = reason.to_string();
            return self.fail(reason, cx);
        }
        let engine = runs.engine().clone();
        let events = engine.subscribe();
        self.turn += 1;
        let turn = self.turn;
        self.state = CommitState::Generating(None);
        cx.notify();
        let (repo, session) = (self.repo.clone(), self.session.clone());
        cx.spawn_in(window, async move |this, cx| {
            let built = cx
                .background_spawn(async move { commit::message_request(&repo, session, ReviewModel::COMMIT_MESSAGE) })
                .await;
            let submitted = this.update(cx, |this, cx| {
                if this.turn != turn {
                    return None;
                }
                let submitted = built
                    .map_err(|error| error.to_string())
                    .and_then(|request| engine.submit(request).map_err(|error| error.to_string()));
                match submitted {
                    Ok(run) => {
                        this.state = CommitState::Generating(Some(run));
                        cx.notify();
                        Some(run)
                    }
                    Err(error) => {
                        this.fail(error, cx);
                        None
                    }
                }
            });
            let Ok(Some(run)) = submitted else { return };
            while let Ok(event) = events.recv().await {
                let RunEvent::Finished { run: ended, state } = event else {
                    continue;
                };
                if ended != run {
                    continue;
                }
                let finished = engine.run(run);
                this.update_in(cx, |this, window, cx| {
                    if this.turn == turn {
                        this.generated(state, finished, window, cx);
                    }
                })
                .ok();
                return;
            }
        })
        .detach();
    }

    fn stop_generating(&mut self, cx: &mut Context<Self>) {
        let CommitState::Generating(run) = self.state else {
            return;
        };
        self.turn += 1;
        self.state = CommitState::Idle;
        if let (Some(run), Some(runs)) = (run, cx.try_global::<Runs>()) {
            runs.engine().stop(run);
        }
        cx.notify();
    }

    fn generated(&mut self, state: RunState, finished: Option<Run>, window: &mut Window, cx: &mut Context<Self>) {
        if state == RunState::Stopped {
            self.state = CommitState::Idle;
            return cx.notify();
        }
        let message = finished.as_ref().and_then(|run| match &run.outcome {
            Some(RunOutcome::Text(text)) if state == RunState::Done => CommitMessage::from_reply(text),
            _ => None,
        });
        let Some(message) = message else {
            let reason = finished.and_then(|run| run.error);
            return self.fail(
                reason.unwrap_or_else(|| "Claude did not return a commit message.".into()),
                cx,
            );
        };
        self.state = CommitState::Idle;
        self.message.update(cx, |input, cx| {
            input.set_value(message.as_str().to_string(), window, cx)
        });
        self.focus(window, cx);
        cx.notify();
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
        let committing = self.state == CommitState::Committing;
        let generating = self.generating();
        let busy = committing || generating;
        let ready = self.parsed(cx).is_some() && !busy;
        let plural = if self.changed == 1 { "file" } else { "files" };
        v_flex()
            .w(px(WIDTH - 24.))
            .p(px(4.))
            .text_size(diffpilot_ui_kit::text_px(13.))
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
                    .text_size(diffpilot_ui_kit::text_px(12.))
                    .text_color(theme.ui.muted)
                    .child(format!(
                        "Stages all {} changed {plural} and commits them.",
                        self.changed
                    )),
            )
            .child(
                div()
                    .mb(px(12.))
                    .child(
                        Input::new(&self.message)
                            .text_size(diffpilot_ui_kit::text_px(13.))
                            .disabled(generating),
                    )
                    .when(self.message.read(cx).value().is_empty(), |field| {
                        field.child(
                            div()
                                .absolute()
                                .top(px(9.))
                                .left(px(13.))
                                .text_size(diffpilot_ui_kit::text_px(13.))
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
                        tag(div(), {
                            let text = text.clone();
                            move || format!("commit:error:{text}")
                        })
                        .mb(px(12.))
                        .min_w_0()
                        .text_size(diffpilot_ui_kit::text_px(12.))
                        .text_color(theme.danger)
                        .child(text),
                    )
                },
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(self.generate_control(committing, theme, cx))
                    .child(div().flex_1())
                    .child(
                        Button::new("cancel-commit")
                            .label("Cancel")
                            .regular()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(CommitEvent::Dismiss))),
                    )
                    .child(
                        tag(
                            Button::new("commit-submit")
                                .label(if committing { "Committing\u{2026}" } else { "Commit" })
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

impl CommitPopover {
    fn generate_control(&self, committing: bool, theme: Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.generating() {
            return h_flex()
                .gap_2()
                .items_center()
                .child(spinner("commit-generating", 13., theme.ui.claude))
                .child(
                    div()
                        .text_size(diffpilot_ui_kit::text_px(12.))
                        .text_color(theme.ui.muted)
                        .child("Writing\u{2026}"),
                )
                .child(
                    tag(Button::new("commit-stop").label("Stop").regular(), || "commit:stop")
                        .on_click(cx.listener(|this, _, _, cx| this.stop_generating(cx))),
                )
                .into_any_element();
        }
        tag(
            Button::new("commit-generate")
                .icon(Glyph::Sparkles, 13.)
                .label("Generate")
                .regular()
                .disabled(committing),
            || "commit:generate",
        )
        .on_click(cx.listener(|this, _, window, cx| this.generate(window, cx)))
        .into_any_element()
    }
}
