use super::rows::{SyncRow, fetch_detail, pull_first_warning, pull_row, push_row, sync_header};
use super::{RemoteStatus, ReviewState, SyncKind, SyncState};
use diffpilot_ui_kit::components::{DISABLED_OPACITY, glyph_icon, spinner, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{Context, Div, Entity, FontWeight, IntoElement, Render, Stateful, Subscription, Window, div, px};
use gpui_component::{h_flex, v_flex};
use std::time::SystemTime;

pub const WIDTH: f32 = 340.0;

pub struct SyncPopover {
    status: Entity<RemoteStatus>,
    _observe: Subscription,
}

impl SyncPopover {
    pub fn new(status: Entity<RemoteStatus>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&status, |_, _, cx| cx.notify());
        Self {
            status,
            _observe: observe,
        }
    }

    fn action_row(
        &self,
        theme: &Theme,
        kind: SyncKind,
        glyph: Glyph,
        row: SyncRow,
        shortcut: &'static str,
        busy: &SyncState,
    ) -> Stateful<Div> {
        let name = kind.name();
        let running = *busy == SyncState::Running(kind);
        let enabled = row.enabled && !matches!(busy, SyncState::Running(_));
        let hover = theme.hover;
        let status = self.status.clone();
        let detail = row.detail.clone();
        tag(h_flex().id(name), move || format!("remote:{name}"))
            .w_full()
            .px(px(10.))
            .py(px(6.))
            .gap(px(10.))
            .rounded(px(8.))
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .on_click(move |_, _, cx| status.update(cx, |status, cx| status.run(kind, cx)))
            })
            .when(!enabled, |this| {
                tag(this.opacity(DISABLED_OPACITY), move || {
                    format!("remote:{name}:disabled")
                })
            })
            .child(if running {
                tag(div(), move || format!("remote:running:{name}"))
                    .child(spinner(("remote-running", 0usize), 15., theme.ui.muted))
                    .into_any_element()
            } else {
                glyph_icon(glyph, 15., theme.ui.text2).into_any_element()
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_size(px(13.)).text_color(theme.ui.text).child(row.title))
                    .child(
                        tag(div(), move || format!("remote:detail:{name}:{detail}"))
                            .text_size(px(12.))
                            .text_color(theme.ui.muted)
                            .child(row.detail),
                    ),
            )
            .child(div().text_size(px(12.)).text_color(theme.ui.faint).child(shortcut))
    }
}

impl Render for SyncPopover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let remote = self.status.read(cx);
        let Some(branch) = remote.branch().cloned() else {
            return div().into_any_element();
        };
        let header = sync_header(&branch);
        let warning = pull_first_warning(&branch);
        let sync = remote.sync().clone();
        let review_failure = match remote.review_state() {
            ReviewState::Failed { number, message } => Some(format!("Could not review #{number}. {message}")),
            _ => None,
        };
        let fetch = SyncRow {
            title: "Fetch".into(),
            detail: fetch_detail(branch.last_fetch, SystemTime::now()),
            enabled: true,
        };
        let pull = pull_row(&branch, remote.pull_mode());
        let push = push_row(&branch);
        let failure = match &sync {
            SyncState::Failed { kind, message } => Some((*kind, message.clone())),
            _ => None,
        };
        v_flex()
            .w(px(WIDTH))
            .gap(px(2.))
            .text_size(px(13.))
            .text_color(theme.ui.text)
            .child(
                tag(div(), {
                    let header = header.clone();
                    move || format!("remote:header:{header}")
                })
                .px(px(10.))
                .pt(px(4.))
                .pb(px(6.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(header),
            )
            .when_some(warning, |this, text| {
                this.child(
                    tag(h_flex(), || "remote:warning")
                        .mx(px(6.))
                        .mb(px(4.))
                        .px(px(8.))
                        .py(px(6.))
                        .gap(px(8.))
                        .items_start()
                        .rounded(px(8.))
                        .bg(theme.amber.opacity(0.14))
                        .text_size(px(12.))
                        .text_color(theme.ui.text)
                        .child(glyph_icon(Glyph::TriangleAlert, 14., theme.amber))
                        .child(div().min_w_0().child(text)),
                )
            })
            .when_some(failure, |this, (kind, message)| {
                this.child(
                    tag(h_flex(), move || format!("remote:error:{}", kind.name()))
                        .mx(px(6.))
                        .mb(px(4.))
                        .px(px(8.))
                        .py(px(6.))
                        .gap(px(8.))
                        .items_start()
                        .text_size(px(12.))
                        .text_color(theme.danger)
                        .child(glyph_icon(Glyph::CircleAlert, 14., theme.danger))
                        .child(div().min_w_0().child(message)),
                )
            })
            .when_some(review_failure, |this, message| {
                this.child(
                    tag(h_flex(), || "remote:review-error")
                        .mx(px(6.))
                        .mb(px(4.))
                        .px(px(8.))
                        .py(px(6.))
                        .gap(px(8.))
                        .items_start()
                        .text_size(px(12.))
                        .text_color(theme.danger)
                        .child(glyph_icon(Glyph::CircleAlert, 14., theme.danger))
                        .child(div().min_w_0().child(message)),
                )
            })
            .child(self.action_row(&theme, SyncKind::Fetch, Glyph::RefreshCw, fetch, "⇧⌘F", &sync))
            .child(self.action_row(&theme, SyncKind::Pull, Glyph::ArrowDown, pull, "⇧⌘L", &sync))
            .child(self.action_row(&theme, SyncKind::Push, Glyph::ArrowUp, push, "⇧⌘U", &sync))
            .into_any_element()
    }
}
