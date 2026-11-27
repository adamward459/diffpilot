use crate::review::card::card_key;
use crate::review::session::ReviewSession;
use diffpilot_core::threads::{ReviewThread, ThreadAnchor, ThreadId};
use diffpilot_ui_kit::components::{Button, Look, OVERLAY_DETAIL_TEXT, OVERLAY_ROW_RADIUS, OVERLAY_TEXT, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FontWeight, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use gpui_component::{Icon, h_flex, v_flex};
use std::collections::{BTreeMap, HashSet};

pub const WIDTH: f32 = 400.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerEvent {
    Dismiss,
}

pub struct FixPicker {
    session: Entity<ReviewSession>,
    excluded: HashSet<ThreadId>,
    _session: Subscription,
}

impl EventEmitter<PickerEvent> for FixPicker {}

impl FixPicker {
    pub fn new(session: Entity<ReviewSession>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        Self {
            session,
            excluded: HashSet::new(),
            _session: subscription,
        }
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.excluded.clear();
        cx.notify();
    }

    fn chosen(&self, fixable: &[ThreadId]) -> Vec<ThreadId> {
        fixable
            .iter()
            .copied()
            .filter(|id| !self.excluded.contains(id))
            .collect()
    }

    fn toggle(&mut self, id: ThreadId, cx: &mut Context<Self>) {
        if !self.excluded.remove(&id) {
            self.excluded.insert(id);
        }
        cx.notify();
    }

    fn toggle_all(&mut self, fixable: &[ThreadId], cx: &mut Context<Self>) {
        if self.chosen(fixable).len() == fixable.len() {
            self.excluded.extend(fixable.iter().copied());
        } else {
            self.excluded.clear();
        }
        cx.notify();
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let chosen = self.chosen(&self.session.read(cx).fixable(cx));
        if chosen.is_empty() {
            return;
        }
        self.session.update(cx, |session, cx| session.fix_threads(&chosen, cx));
        cx.emit(PickerEvent::Dismiss);
    }

    fn row(&self, thread: &ReviewThread, theme: Theme, cx: &Context<Self>) -> AnyElement {
        let id = thread.id;
        let on = !self.excluded.contains(&id);
        let label = match &thread.anchor {
            ThreadAnchor::Line { line, .. } => format!("L{line}"),
            ThreadAnchor::File { .. } => "File".to_string(),
            ThreadAnchor::Change => "General".to_string(),
        };
        let text = thread
            .sent_comments()
            .last()
            .map(|comment| comment.body.clone())
            .unwrap_or_default();
        let checkbox = div()
            .flex_none()
            .mt(px(1.))
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .border_1()
            .when(on, |box_| {
                box_.border_color(theme.accent)
                    .bg(theme.accent)
                    .child(Icon::new(Glyph::Check).size(px(12.)).text_color(theme.on_accent))
            })
            .when(!on, |box_| box_.border_color(theme.ui.faint));
        tag(
            h_flex().id(("fix-row", id.0 as usize)).gap_2().px_2().py(px(5.)),
            || format!("fix:row:{}", card_key(thread)),
        )
        .items_start()
        .rounded(px(OVERLAY_ROW_RADIUS))
        .cursor_pointer()
        .hover(|row| row.bg(theme.hover))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(id, cx)))
        .child(checkbox)
        .child(
            div()
                .flex_none()
                .w(px(52.))
                .text_size(diffpilot_ui_kit::text_px(OVERLAY_DETAIL_TEXT))
                .text_color(theme.ui.muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(diffpilot_ui_kit::text_px(OVERLAY_TEXT))
                .line_height(px(17.))
                .line_clamp(2)
                .text_color(theme.ui.text)
                .child(text),
        )
        .into_any_element()
    }
}

impl Render for FixPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let session = self.session.read(cx);
        let fixable = session.fixable(cx);
        let blocker = session.fix_blocker(cx);
        let mut groups: BTreeMap<&str, Vec<&ReviewThread>> = BTreeMap::new();
        for id in &fixable {
            if let Some(thread) = session.thread(*id) {
                groups.entry(thread.anchor.path()).or_default().push(thread);
            }
        }
        let count = self.chosen(&fixable).len();
        let all = count == fixable.len();
        let groups: Vec<AnyElement> = groups
            .into_iter()
            .map(|(path, threads)| {
                v_flex()
                    .gap(px(1.))
                    .child(
                        div()
                            .px_2()
                            .pt(px(6.))
                            .pb(px(2.))
                            .text_size(diffpilot_ui_kit::text_px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.ui.text2)
                            .child(if path.is_empty() { "General" } else { path }.to_string()),
                    )
                    .children(threads.into_iter().map(|thread| self.row(thread, theme, cx)))
                    .into_any_element()
            })
            .collect();
        let blocked = blocker.is_some();
        v_flex()
            .w(px(WIDTH - 24.))
            .p(px(4.))
            .text_size(diffpilot_ui_kit::text_px(13.))
            .text_color(theme.ui.text)
            .child(
                h_flex()
                    .gap_2()
                    .mb(px(8.))
                    .items_center()
                    .child(Icon::new(Glyph::Sparkles).size(px(13.)).text_color(theme.ui.claude))
                    .child(div().font_weight(FontWeight::BOLD).child("Fix comments"))
                    .child(div().flex_1())
                    .child(
                        tag(
                            div()
                                .id("fix-toggle-all")
                                .cursor_pointer()
                                .text_size(diffpilot_ui_kit::text_px(12.))
                                .text_color(theme.ui.muted)
                                .hover(|label| label.text_color(theme.ui.text))
                                .child(if all { "Select none" } else { "Select all" }),
                            || "fix:all",
                        )
                        .on_click(cx.listener({
                            let fixable = fixable.clone();
                            move |this, _, _, cx| this.toggle_all(&fixable, cx)
                        })),
                    ),
            )
            .child(
                v_flex()
                    .id("fix-rows")
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .children(groups),
            )
            .child(div().h(px(1.)).bg(theme.ui.line).my(px(10.)))
            .when_some(blocker.clone(), |col, message| {
                col.child(
                    div()
                        .mb(px(8.))
                        .text_size(diffpilot_ui_kit::text_px(12.))
                        .text_color(theme.amber)
                        .child(message),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1())
                    .child(
                        tag(Button::new("fix-cancel").label("Cancel").regular(), || "fix:cancel")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(PickerEvent::Dismiss))),
                    )
                    .child(
                        tag(
                            Button::new("fix-send")
                                .look(Look::Claude)
                                .icon(Glyph::Sparkles, 12.)
                                .label(format!("Send {count} to Claude"))
                                .regular()
                                .disabled(count == 0 || blocked),
                            || "fix:send",
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.send(cx))),
                    ),
            )
    }
}
