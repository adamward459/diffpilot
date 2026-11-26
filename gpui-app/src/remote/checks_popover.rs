use super::rows::{JobGroup, JobRow, job_groups};
use super::{RemoteStatus, RerunState, open_in_browser};
use diffpilot_ui_kit::components::{Button, Look, glyph_icon, status_dot, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, IntoElement, Render, Subscription, Window, div, px};
use gpui_component::{h_flex, v_flex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const WIDTH: f32 = 440.0;

pub struct ChecksPopover {
    status: Entity<RemoteStatus>,
    _observe: Subscription,
}

impl ChecksPopover {
    pub fn new(status: Entity<RemoteStatus>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&status, |_, _, cx| cx.notify());
        Self {
            status,
            _observe: observe,
        }
    }

    fn job_row(&self, theme: &Theme, job: JobRow, busy: bool) -> impl IntoElement + use<> {
        let name = job.name.clone();
        let dot = job.tone;
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
            .when_some(job.duration, |this, text| {
                this.child(
                    tag(div(), {
                        let (name, text) = (name.clone(), text.clone());
                        move || format!("checks:duration:{name}:{text}")
                    })
                    .text_size(px(12.))
                    .text_color(theme.ui.muted)
                    .child(text.clone()),
                )
            })
            .when_some(job.rerun, |this, job_id| {
                this.child(
                    tag(
                        Button::new(("rerun-job", job_id as usize))
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

    fn group(&self, theme: &Theme, group: JobGroup, busy: bool) -> impl IntoElement + use<> {
        let title = group.title.clone();
        v_flex()
            .child(
                tag(div(), move || format!("checks:group:{title}"))
                    .px(px(10.))
                    .pt(px(8.))
                    .pb(px(2.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.ui.muted)
                    .child(group.title),
            )
            .children(group.jobs.into_iter().map(|job| self.job_row(theme, job, busy)))
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
        let number = remote.branch_pr().map(|pr| pr.number);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0));
        let groups = remote.checks().map(|checks| job_groups(checks, now));
        let has_failed = remote
            .checks()
            .is_some_and(|checks| !checks.failed_run_ids().is_empty());
        let busy = remote.rerun_state() == &RerunState::Running;
        let error = match remote.rerun_state() {
            RerunState::Failed(message) => Some(message.clone()),
            _ => None,
        };
        let status = self.status.clone();
        let header = number.map_or_else(|| "Checks".to_string(), |number| format!("Checks · PR #{number}"));
        v_flex()
            .w(px(WIDTH))
            .text_size(px(13.))
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
                                .disabled(!has_failed || busy),
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
                        .text_size(px(12.))
                        .text_color(theme.danger)
                        .child(glyph_icon(Glyph::CircleAlert, 14., theme.danger))
                        .child(div().min_w_0().child(message)),
                )
            })
            .child(match groups {
                Some(groups) if groups.is_empty() => note(&theme, "No checks ran for this commit."),
                Some(groups) => v_flex()
                    .id("checks-list")
                    .max_h(px(380.))
                    .overflow_y_scroll()
                    .children(groups.into_iter().map(|group| self.group(&theme, group, busy)))
                    .into_any_element(),
                None => note(&theme, "Loading checks…"),
            })
    }
}

fn note(theme: &Theme, text: &'static str) -> gpui::AnyElement {
    div()
        .px(px(10.))
        .py(px(6.))
        .text_size(px(12.))
        .text_color(theme.ui.muted)
        .child(text)
        .into_any_element()
}
