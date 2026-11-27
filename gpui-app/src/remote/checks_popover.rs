use super::rows::{duration_text, outcome_tone};
use super::{RemoteStatus, RerunState, open_in_browser};
use diffpilot_core::forge::{Checks, Job, Stage};
use diffpilot_ui_kit::components::scroll_y;
use diffpilot_ui_kit::components::{Button, Look, glyph_icon, status_dot, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::ScrollHandle;
use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, IntoElement, Render, Subscription, Window, div, px};
use gpui_component::{h_flex, v_flex};

pub const WIDTH: f32 = 440.0;

pub struct ChecksPopover {
    scroll: ScrollHandle,
    status: Entity<RemoteStatus>,
    _observe: Subscription,
}

impl ChecksPopover {
    pub fn new(status: Entity<RemoteStatus>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&status, |_, _, cx| cx.notify());
        Self {
            scroll: ScrollHandle::new(),
            status,
            _observe: observe,
        }
    }

    fn job_row(&self, theme: &Theme, job: Job, busy: bool) -> impl IntoElement + use<> {
        let name = job.name.clone();
        let dot = outcome_tone(job.outcome);
        let status = self.status.clone();
        h_flex()
            .gap(px(8.))
            .px(px(10.))
            .py(px(4.))
            .items_center()
            .child(tag(status_dot(dot.color(theme)), {
                let name = name.clone();
                move || format!("checks:dot:{name}:{}", dot.name())
            }))
            .child(
                tag(div(), {
                    let name = name.clone();
                    move || format!("checks:job:{name}")
                })
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(job.name),
            )
            .when_some(job.duration_seconds.map(duration_text), |this, text| {
                this.child(
                    tag(div(), {
                        let (name, text) = (name.clone(), text.clone());
                        move || format!("checks:duration:{name}:{text}")
                    })
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                    .text_color(theme.ui.muted)
                    .child(text.clone()),
                )
            })
            .when_some(job.rerun, |this, job_id| {
                this.child(
                    tag(
                        Button::new(("rerun-job", job_id.0 as usize))
                            .look(Look::Ghost)
                            .label("Re-run")
                            .disabled(busy),
                        {
                            let name = name.clone();
                            move || format!("checks:rerun:{name}")
                        },
                    )
                    .on_click(move |_, _, cx| status.update(cx, |status, cx| status.rerun_job(job_id, cx))),
                )
            })
            .when_some(job.log_url, |this, url| {
                this.child(
                    tag(
                        Button::new(("open-log", hash(&name)))
                            .look(Look::Ghost)
                            .label("Open Log"),
                        {
                            let name = name.clone();
                            move || format!("checks:log:{name}")
                        },
                    )
                    .on_click(move |_, _, cx| open_in_browser(url.clone(), cx)),
                )
            })
    }

    fn stage(&self, theme: &Theme, stage: Stage, busy: bool) -> impl IntoElement + use<> {
        let title = stage.name.clone();
        v_flex()
            .child(
                tag(div(), move || format!("checks:group:{title}"))
                    .px(px(10.))
                    .pt(px(8.))
                    .pb(px(2.))
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.ui.muted)
                    .child(stage.name),
            )
            .children(stage.jobs.into_iter().map(|job| self.job_row(theme, job, busy)))
    }
}

fn hash(text: &str) -> usize {
    text.bytes()
        .fold(5381usize, |hash, byte| hash.wrapping_mul(33) ^ usize::from(byte))
}

impl Render for ChecksPopover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let remote = self.status.read(cx);
        let label = remote
            .branch_pr()
            .zip(remote.words())
            .map(|(change, words)| words.label(change.number));
        let stages = remote.checks().map(|checks| checks.stages.clone());
        let can_rerun = remote.checks().is_some_and(Checks::can_rerun_failed);
        let busy = remote.rerun_state() == &RerunState::Running;
        let error = match remote.rerun_state() {
            RerunState::Failed(message) => Some(message.clone()),
            _ => None,
        };
        let status = self.status.clone();
        let header = label.map_or_else(|| "Checks".to_string(), |label| format!("Checks · {label}"));
        v_flex()
            .w(px(WIDTH))
            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
            .text_color(theme.ui.text)
            .child(
                h_flex()
                    .px(px(10.))
                    .pt(px(4.))
                    .pb(px(6.))
                    .gap(px(8.))
                    .items_center()
                    .child(
                        tag(div(), {
                            let header = header.clone();
                            move || format!("checks:header:{header}")
                        })
                        .flex_1()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(header),
                    )
                    .child(
                        tag(
                            Button::new("rerun-failed")
                                .look(Look::Plain)
                                .icon(Glyph::RefreshCw, 12.)
                                .label("Re-run Failed")
                                .disabled(!can_rerun || busy),
                            || "checks:rerun-failed",
                        )
                        .on_click(move |_, _, cx| status.update(cx, |status, cx| status.rerun_failed(cx))),
                    ),
            )
            .when_some(error, |this, message| {
                this.child(
                    tag(h_flex(), || "checks:error")
                        .mx(px(10.))
                        .mb(px(4.))
                        .gap(px(8.))
                        .items_start()
                        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                        .text_color(theme.danger)
                        .child(glyph_icon(Glyph::CircleAlert, 14., theme.danger))
                        .child(div().min_w_0().child(message)),
                )
            })
            .child(match stages {
                Some(stages) if stages.is_empty() => note(&theme, "No checks ran for this commit."),
                Some(stages) => scroll_y(
                    &self.scroll,
                    v_flex()
                        .id("checks-list")
                        .children(stages.into_iter().map(|stage| self.stage(&theme, stage, busy))),
                )
                .max_h(px(380.))
                .into_any_element(),
                None => note(&theme, "Loading checks…"),
            })
    }
}

fn note(theme: &Theme, text: &'static str) -> gpui::AnyElement {
    div()
        .px(px(10.))
        .py(px(6.))
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
        .text_color(theme.ui.muted)
        .child(text)
        .into_any_element()
}
