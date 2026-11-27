use diffpilot_core::run::{Run, RunKind};
use diffpilot_ui_kit::components::{Button, Look, Presence, dialog, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{App, Context, EventEmitter, FocusHandle, FontWeight, KeyBinding, SharedString, Window, actions, div, px};
use gpui_component::{Icon, h_flex, v_flex};

const WIDTH: f32 = 420.;
const BASE_HEIGHT: f32 = 188.;
const RUN_HEIGHT: f32 = 20.;
const KEPT_HEIGHT: f32 = 36.;
const CONTEXT: &str = "SwitchAlert";

actions!(diffpilot_switch, [ConfirmSwitch, CancelSwitch]);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", ConfirmSwitch, Some(CONTEXT)),
        KeyBinding::new("escape", CancelSwitch, Some(CONTEXT)),
    ]);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchEvent {
    StopAndSwitch { ask_again: bool },
    Cancel,
}

pub struct SwitchAlert {
    target: String,
    kinds: Vec<RunKind>,
    ask_again: bool,
    presence: Presence,
    focus: FocusHandle,
}

impl EventEmitter<SwitchEvent> for SwitchAlert {}

impl SwitchAlert {
    pub fn new(target: String, runs: &[Run], window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut presence = Presence::new(false);
        presence.set_open(true, cx, |this: &mut Self| &mut this.presence);
        Self {
            target,
            kinds: runs.iter().map(|run| run.kind).collect(),
            ask_again: true,
            presence,
            focus,
        }
    }

    pub fn is_mounted(&self) -> bool {
        self.presence.is_mounted()
    }

    fn keeps_changes(&self) -> bool {
        self.kinds.iter().any(|kind| kind.activity().edits())
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        self.presence.set_open(false, cx, |this| &mut this.presence);
        cx.emit(SwitchEvent::StopAndSwitch {
            ask_again: self.ask_again,
        });
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.presence.is_open() {
            return;
        }
        self.presence.set_open(false, cx, |this| &mut this.presence);
        cx.emit(SwitchEvent::Cancel);
    }
}

pub fn title(target: &str, count: usize) -> String {
    match count {
        1 => format!("Stop Claude and switch to {target}?"),
        _ => format!("Stop {count} Claude runs and switch?"),
    }
}

impl Render for SwitchAlert {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Theme::of(cx);
        let keeps = self.keeps_changes();
        let height = BASE_HEIGHT + RUN_HEIGHT * self.kinds.len() as f32 + if keeps { KEPT_HEIGHT } else { 0. };
        let runs = self.kinds.iter().map(|kind| {
            h_flex()
                .gap(px(8.))
                .child(div().w(px(4.)).h(px(4.)).rounded_full().bg(tokens.ui.muted))
                .child(SharedString::from(kind.activity().running_label(None)))
        });
        let ask_again = self.ask_again;
        let checkbox = div()
            .flex_none()
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .border_1()
            .when(!ask_again, |box_| {
                box_.border_color(tokens.accent)
                    .bg(tokens.accent)
                    .child(Icon::new(Glyph::Check).size(px(12.)).text_color(tokens.on_accent))
            })
            .when(ask_again, |box_| box_.border_color(tokens.ui.faint));
        let content = v_flex()
            .id("switch-alert-content")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .p(px(20.))
            .gap(px(12.))
            .on_action(cx.listener(|this, _: &ConfirmSwitch, _, cx| this.confirm(cx)))
            .on_action(cx.listener(|this, _: &CancelSwitch, _, cx| this.cancel(cx)))
            .child(
                tag(
                    div()
                        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::TITLE))
                        .font_weight(FontWeight::SEMIBOLD),
                    || "switch-alert:title",
                )
                .child(SharedString::from(title(&self.target, self.kinds.len()))),
            )
            .child(
                v_flex()
                    .gap(px(4.))
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                    .text_color(tokens.ui.text2)
                    .child("Claude is working here:")
                    .children(runs),
            )
            .when(keeps, |this| {
                this.child(
                    div()
                        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                        .text_color(tokens.ui.text2)
                        .child("Changes Claude made so far stay in your working tree."),
                )
            })
            .child(
                tag(
                    h_flex()
                        .id("switch-alert-ask")
                        .gap(px(8.))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ask_again = !this.ask_again;
                            cx.notify();
                        }))
                        .child(checkbox)
                        .child("Don't ask again"),
                    || "switch-alert:ask-again",
                )
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                .text_color(tokens.ui.text2),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap(px(8.))
                    .justify_end()
                    .child(tag(
                        Button::new("switch-alert-cancel")
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                        || "switch-alert:cancel",
                    ))
                    .child(tag(
                        Button::new("switch-alert-confirm")
                            .look(Look::Primary)
                            .label("Stop and Switch")
                            .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                        || "switch-alert:confirm",
                    )),
            );
        dialog("switch-alert", &self.presence)
            .tags("switch-alert:backdrop", "switch-alert:dialog")
            .size(WIDTH, height)
            .on_backdrop_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_names_the_folder_for_one_run_and_counts_more() {
        assert_eq!(title("alpha", 1), "Stop Claude and switch to alpha?");
        assert_eq!(title("alpha", 3), "Stop 3 Claude runs and switch?");
    }
}
