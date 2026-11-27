use super::checks_popover::ChecksPopover;
use super::rows::{Tone, checks_detail, outcome_tone, overall_tone, review_badge};
use super::sync_popover::SyncPopover;
use super::{RemoteStatus, SyncState};
use diffpilot_ui_kit::components::popover;
use diffpilot_ui_kit::components::{Button, Look, activity_capsule, glyph_icon, spinner, status_dot, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{Context, Corner, Entity, IntoElement, Render, Subscription, Window, div, px};
use gpui_component::h_flex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    Closed,
    Sync,
    Checks,
}

pub struct RemoteCapsule {
    status: Entity<RemoteStatus>,
    sync: Entity<SyncPopover>,
    checks: Entity<ChecksPopover>,
    open: Open,
    _observe: Subscription,
}

impl RemoteCapsule {
    pub fn new(status: Entity<RemoteStatus>, cx: &mut Context<Self>) -> Self {
        let sync = cx.new(|cx| SyncPopover::new(status.clone(), cx));
        let checks = cx.new(|cx| ChecksPopover::new(status.clone(), cx));
        let observe = cx.observe(&status, |_, _, cx| cx.notify());
        Self {
            status,
            sync,
            checks,
            open: Open::Closed,
            _observe: observe,
        }
    }

    fn set_open(&mut self, part: Open, open: bool, cx: &mut Context<Self>) {
        let next = if open {
            part
        } else if self.open == part {
            Open::Closed
        } else {
            self.open
        };
        if next != self.open {
            self.open = next;
            cx.notify();
        }
    }

    fn part(id: &'static str, tag_name: &'static str) -> Button {
        tag(
            Button::new(id)
                .look(Look::Ghost)
                .regular()
                .rounded_full()
                .h(px(30.))
                .px(px(10.))
                .gap(px(8.)),
            move || tag_name,
        )
    }
}

impl Render for RemoteCapsule {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let remote = self.status.read(cx);
        let Some(branch) = remote.branch().cloned() else {
            return div().into_any_element();
        };
        let failed = matches!(remote.sync(), SyncState::Failed { .. });
        let running = matches!(remote.sync(), SyncState::Running(_));
        let pr = remote.branch_pr().cloned();
        let checks = remote.checks().cloned();

        let counts = match branch.upstream {
            Some(_) => {
                let (ahead, behind) = (branch.ahead, branch.behind);
                let count = |glyph: Glyph, value: u32| {
                    h_flex()
                        .gap(px(2.))
                        .child(glyph_icon(glyph, 12., theme.ui.muted))
                        .child(value.to_string())
                };
                tag(h_flex().gap(px(8.)), move || format!("remote:counts:{ahead}:{behind}"))
                    .child(count(Glyph::ArrowUp, ahead))
                    .child(count(Glyph::ArrowDown, behind))
                    .into_any_element()
            }
            None => tag(div(), || "remote:no-upstream")
                .text_color(theme.ui.muted)
                .child("No upstream")
                .into_any_element(),
        };
        let sync_trigger = Self::part("remote-status", "remote:status")
            .when(running, |this| {
                this.child(spinner(("remote-busy", 0usize), 13., theme.ui.muted))
            })
            .when(failed, |this| {
                this.child(tag(div(), || "remote:failed").child(glyph_icon(Glyph::TriangleAlert, 14., theme.amber)))
            })
            .child(counts)
            .tooltip("Branch status");

        let view = cx.entity().downgrade();
        let on_sync = {
            let view = view.clone();
            move |open: &bool, _: &mut Window, cx: &mut gpui::App| {
                let open = *open;
                view.update(cx, |this, cx| this.set_open(Open::Sync, open, cx)).ok();
            }
        };
        let on_checks = move |open: &bool, _: &mut Window, cx: &mut gpui::App| {
            let open = *open;
            view.update(cx, |this, cx| this.set_open(Open::Checks, open, cx)).ok();
        };

        let checks_part = pr.as_ref().map(|pr| {
            let tone = checks
                .as_ref()
                .and_then(|checks| overall_tone(checks.overall()))
                .or_else(|| pr.checks.map(outcome_tone));
            let detail = checks
                .as_ref()
                .and_then(|checks| checks_detail(checks.counts(), checks.overall()));
            let trigger = Self::part("remote-checks", "remote:checks")
                .children(tone.map(|tone| {
                    tag(status_dot(tone.color(&theme)), move || {
                        format!("remote:checks-dot:{}", tone.name())
                    })
                }))
                .child("Checks")
                .children(detail.map(|text| {
                    tag(div(), {
                        let text = text.clone();
                        move || format!("remote:checks-detail:{text}")
                    })
                    .text_color(theme.ui.muted)
                    .child(text)
                }))
                .tooltip("Checks");
            let content = self.checks.clone();
            popover("remote-checks-popover")
                .anchor(Corner::TopRight)
                .open(self.open == Open::Checks)
                .on_open_change(on_checks)
                .trigger(trigger)
                .content(move |_, _, _| content.clone())
        });
        let review = pr
            .as_ref()
            .and_then(|pr| review_badge(pr.review))
            .map(|(text, tone): (&'static str, Tone)| {
                tag(div(), move || format!("remote:review:{text}"))
                    .px(px(6.))
                    .text_color(tone.color(&theme))
                    .child(text)
            });

        let sync_content = self.sync.clone();
        activity_capsule(&theme)
            .child(
                popover("remote-sync-popover")
                    .anchor(Corner::TopRight)
                    .open(self.open == Open::Sync)
                    .on_open_change(on_sync)
                    .trigger(sync_trigger)
                    .content(move |_, _, _| sync_content.clone()),
            )
            .children(checks_part)
            .children(review)
            .into_any_element()
    }
}
