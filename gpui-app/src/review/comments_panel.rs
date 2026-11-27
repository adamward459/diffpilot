use crate::review::session::ReviewSession;
use diffpilot_core::store::{SessionId, SessionThreads, Store};
use diffpilot_core::thread_groups::{FileGroup, Grouped, Row, StatusFilter, ViewGroup, group_threads, line_label};
use diffpilot_core::threads::{CommentSource, ReviewThread, ThreadId, ThreadStatus};
use diffpilot_ui_kit::components::scroll_y;
use diffpilot_ui_kit::components::{
    Button, Look, Presence, avatar, bar_button, glyph_icon, label_switch, side_sheet, tag,
};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::ScrollHandle;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, px,
};
use gpui_component::{h_flex, v_flex};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

pub const WIDTH: f32 = 460.0;
const INSET: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentsEvent {
    Close,
    Focus(ThreadId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    View,
    All,
}

pub struct CommentsPanel {
    scroll: ScrollHandle,
    review: Entity<ReviewSession>,
    store: Arc<Store>,
    scope: Scope,
    status: StatusFilter,
    expanded: HashSet<ThreadId>,
    hovered: Option<ThreadId>,
    others: Vec<SessionThreads>,
    focus: FocusHandle,
    presence: Presence,
    _review: Subscription,
}

impl EventEmitter<CommentsEvent> for CommentsPanel {}

impl Focusable for CommentsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// How many threads of this view are open, as the panel counts them.
pub fn open_count(session: &ReviewSession) -> usize {
    group_threads(&[current_threads(session)], session.session_id(), StatusFilter::Open)
        .counts
        .open
}

fn current_threads(session: &ReviewSession) -> SessionThreads {
    SessionThreads {
        session: session.session_id(),
        key: session.session_key(),
        latest: i64::MAX,
        threads: session.all_threads().cloned().collect(),
    }
}

fn empty_message(status: StatusFilter, total: usize, scope: Scope) -> &'static str {
    match (total, scope, status) {
        (0, Scope::View, _) => "No comments in this view yet. Switch to All views to see comments from other views.",
        (0, Scope::All, _) => {
            "No comments in this repository yet. Comments you or Claude leave in any view show up here."
        }
        (_, _, StatusFilter::Open) => "No open comments. Everything has been resolved.",
        (_, _, StatusFilter::Resolved) => "No resolved comments.",
        (_, _, StatusFilter::All) => "No comments match these filters.",
    }
}

impl CommentsPanel {
    pub fn new(review: Entity<ReviewSession>, store: Arc<Store>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&review, |this, _, cx| {
            this.reload(cx);
            cx.notify();
        });
        Self {
            scroll: ScrollHandle::new(),
            review,
            store,
            scope: Scope::View,
            status: StatusFilter::Open,
            expanded: HashSet::new(),
            hovered: None,
            others: Vec::new(),
            focus: cx.focus_handle(),
            presence: Presence::new(false),
            _review: subscription,
        }
    }

    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reload(cx);
        window.focus(&self.focus);
        self.presence.set_open(true, cx, |this| &mut this.presence);
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.presence.set_open(false, cx, |this| &mut this.presence);
    }

    pub fn is_open(&self) -> bool {
        self.presence.is_open()
    }

    fn reload(&mut self, cx: &App) {
        if self.scope == Scope::View {
            self.others.clear();
            return;
        }
        let session = self.review.read(cx);
        let (current, repo) = (session.session_id(), session.session_key().repo);
        self.others = self
            .store
            .load_repo_threads(&repo)
            .unwrap_or_default()
            .into_iter()
            .filter(|other| other.session != current)
            .collect();
    }

    fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.scope = scope;
        self.reload(cx);
        cx.notify();
    }

    fn set_status(&mut self, status: StatusFilter, cx: &mut Context<Self>) {
        self.status = status;
        cx.notify();
    }

    fn grouped(&self, cx: &App) -> Grouped {
        let session = self.review.read(cx);
        let mut sessions = vec![current_threads(session)];
        sessions.extend(self.others.iter().cloned());
        group_threads(&sessions, session.session_id(), self.status)
    }

    fn act(
        &mut self,
        id: ThreadId,
        change: impl FnOnce(&mut ReviewSession, &mut Context<ReviewSession>),
        cx: &mut Context<Self>,
    ) {
        self.review.update(cx, |session, cx| change(session, cx));
        if self.hovered == Some(id) {
            self.hovered = None;
        }
        cx.notify();
    }

    fn click_row(&mut self, row: &Row, current: SessionId, cx: &mut Context<Self>) {
        let id = row.thread.id;
        if row.session == current && row.thread.status == ThreadStatus::Open {
            cx.emit(CommentsEvent::Focus(id));
        } else if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
        cx.notify();
    }

    fn header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let scope = {
            let view = cx.entity().downgrade();
            label_switch(
                theme,
                "comments-scope",
                &["This view", "All views"],
                usize::from(self.scope == Scope::All),
                |index| format!("comments:scope:{}", ["view", "all"][index]),
                Rc::new(move |index, _, cx| {
                    let scope = if index == 0 { Scope::View } else { Scope::All };
                    view.update(cx, |this, cx| this.set_scope(scope, cx)).ok();
                }),
            )
        };
        h_flex()
            .h(px(52.))
            .flex_none()
            .px(px(16.))
            .gap(px(8.))
            .child(glyph_icon(Glyph::MessageSquare, 16., theme.ui.text2))
            .child(
                div()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.ui.text)
                    .child("Comments"),
            )
            .child(scope)
            .child(div().flex_1())
            .child(tag(
                bar_button(
                    theme,
                    "comments-close",
                    Glyph::X,
                    "Close",
                    true,
                    false,
                    cx.listener(|_, _, _, cx| cx.emit(CommentsEvent::Close)),
                ),
                || "comments:close",
            ))
            .into_any_element()
    }

    fn status_row(&self, theme: &Theme, grouped: &Grouped, cx: &mut Context<Self>) -> AnyElement {
        let counts = grouped.counts;
        let view = cx.entity().downgrade();
        let labels = [
            format!("Open {}", counts.open),
            format!("Resolved {}", counts.resolved),
            format!("All {}", counts.all),
        ];
        h_flex()
            .flex_none()
            .px(px(16.))
            .pb(px(12.))
            .border_b_1()
            .border_color(theme.ui.line)
            .child(label_switch(
                theme,
                "comments-status",
                &labels,
                match self.status {
                    StatusFilter::Open => 0,
                    StatusFilter::Resolved => 1,
                    StatusFilter::All => 2,
                },
                |index| format!("comments:status:{}", ["open", "resolved", "all"][index]),
                Rc::new(move |index, _, cx| {
                    let status = [StatusFilter::Open, StatusFilter::Resolved, StatusFilter::All][index];
                    view.update(cx, |this, cx| this.set_status(status, cx)).ok();
                }),
            ))
            .into_any_element()
    }

    fn view_header(&self, theme: &Theme, view: &ViewGroup) -> AnyElement {
        h_flex()
            .h(px(32.))
            .flex_none()
            .px(px(16.))
            .gap(px(6.))
            .child(glyph_icon(Glyph::GitBranch, 14., theme.ui.muted))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.ui.text)
                    .child(diffpilot_core::thread_groups::view_label(&view.key)),
            )
            .when(view.current, |this| {
                this.child(pill("This view", theme.chrome.badge_fill, theme.chrome.secondary))
            })
            .child(div().flex_1())
            .child(
                div()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                    .text_color(theme.ui.muted)
                    .child(view.count.to_string()),
            )
            .into_any_element()
    }

    fn file_group(&self, theme: &Theme, file: &FileGroup, current: SessionId, cx: &mut Context<Self>) -> AnyElement {
        let title = if file.path.is_empty() {
            "General comments".to_string()
        } else {
            file.path.clone()
        };
        v_flex()
            .px(px(12.))
            .child(
                div()
                    .px(px(4.))
                    .pt(px(8.))
                    .pb(px(6.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .font_family(cx.global::<gpui_component::Theme>().mono_font_family.clone())
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                    .text_color(theme.ui.text2)
                    .child(title),
            )
            .child(
                v_flex()
                    .gap(px(8.))
                    .children(file.rows.iter().map(|row| self.row(theme, row, current, cx))),
            )
            .into_any_element()
    }

    fn row(&self, theme: &Theme, row: &Row, current: SessionId, cx: &mut Context<Self>) -> AnyElement {
        let thread = &row.thread;
        let first = &thread.comments[0];
        let id = thread.id;
        let editable = row.session == current;
        let open = thread.status == ThreadStatus::Open;
        let expanded = self.expanded.contains(&id);
        let actions = editable && self.hovered == Some(id);
        let claude = first.source == CommentSource::Claude;
        let replies = thread.comments.len() - 1;
        let name = first.body.clone();
        let hover = theme.hover;
        let header = h_flex()
            .gap(px(6.))
            .h(px(22.))
            .children(line_label(thread).map(|label| {
                div()
                    .flex_none()
                    .font_family(cx.global::<gpui_component::Theme>().mono_font_family.clone())
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                    .text_color(theme.ui.text2)
                    .child(label)
            }))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                    .text_color(theme.ui.text2)
                    .child(first.author.clone()),
            )
            .children(thread.finding.as_ref().map(|finding| {
                let color = theme.severity(finding.severity);
                pill(finding.severity.title(), color.opacity(0.16), color)
            }))
            .when(!open, |this| {
                let label = if thread.status == ThreadStatus::Dismissed {
                    "Dismissed"
                } else {
                    "Resolved"
                };
                this.child(pill(label, theme.chrome.badge_fill, theme.chrome.secondary))
            })
            .child(div().flex_1())
            .when(actions, |this| {
                let toggle = if open {
                    Button::new(("comments-resolve", id.0 as usize))
                        .look(Look::Ghost)
                        .icon(Glyph::Check, 13.)
                        .label("Resolve")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.act(id, |session, cx| session.resolve_thread(id, cx), cx);
                        }))
                } else {
                    Button::new(("comments-reopen", id.0 as usize))
                        .look(Look::Ghost)
                        .icon(Glyph::Undo2, 13.)
                        .label("Reopen")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.act(id, |session, cx| session.reopen_thread(id, cx), cx);
                        }))
                };
                let verb = if open { "resolve" } else { "reopen" };
                this.child(tag(toggle, {
                    let name = name.clone();
                    move || format!("comments:{verb}:{name}")
                }))
                .child(tag(
                    Button::new(("comments-delete", id.0 as usize))
                        .look(Look::Ghost)
                        .icon_color(Glyph::X, 13., theme.danger)
                        .tooltip("Delete this comment and its replies")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.act(id, |session, cx| session.remove_thread(id, cx), cx);
                        })),
                    {
                        let name = name.clone();
                        move || format!("comments:delete:{name}")
                    },
                ))
            });
        let body = if expanded {
            self.conversation(theme, thread, cx)
        } else {
            v_flex()
                .gap(px(4.))
                .child(
                    div()
                        .line_clamp(2)
                        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                        .line_height(px(18.))
                        .text_color(theme.ui.text)
                        .child(first.body.clone()),
                )
                .when(replies > 0, |this| {
                    this.child(
                        div()
                            .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                            .text_color(theme.ui.muted)
                            .child(format!("{replies} {}", if replies == 1 { "reply" } else { "replies" })),
                    )
                })
                .into_any_element()
        };
        let row = row.clone();
        tag(h_flex().id(("comments-row", id.0 as usize)), {
            let name = name.clone();
            move || format!("comments:row:{name}")
        })
        .items_start()
        .gap(px(10.))
        .px(px(12.))
        .py(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme.ui.line)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            let next = hovered.then_some(id);
            if this.hovered != next && (*hovered || this.hovered == Some(id)) {
                this.hovered = next;
                cx.notify();
            }
        }))
        .on_click(cx.listener(move |this, _, _, cx| this.click_row(&row, current, cx)))
        .child(avatar(&first.author, claude, theme, 20.))
        .child(v_flex().flex_1().min_w_0().gap(px(2.)).child(header).child(body))
        .into_any_element()
    }

    fn conversation(&self, theme: &Theme, thread: &ReviewThread, cx: &App) -> AnyElement {
        let mono = cx.global::<gpui_component::Theme>().mono_font_family.clone();
        let name = thread.comments[0].body.clone();
        tag(
            v_flex()
                .mt(px(4.))
                .rounded(px(8.))
                .border_1()
                .border_color(theme.ui.line)
                .bg(theme.ui.raised)
                .overflow_hidden(),
            move || format!("comments:conversation:{name}"),
        )
        .children(thread.anchor_text.as_ref().map(|text| {
            div()
                .px(px(10.))
                .py(px(6.))
                .max_h(px(96.))
                .overflow_hidden()
                .border_b_1()
                .border_color(theme.ui.line)
                .font_family(mono.clone())
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
                .text_color(theme.ui.muted)
                .child(text.clone())
        }))
        .children(thread.comments.iter().enumerate().map(|(index, comment)| {
            v_flex()
                .px(px(10.))
                .py(px(8.))
                .gap(px(2.))
                .when(index > 0, |this| this.border_t_1().border_color(theme.ui.line))
                .child(
                    h_flex()
                        .gap(px(5.))
                        .when(comment.source == CommentSource::Claude, |this| {
                            this.child(glyph_icon(Glyph::Sparkles, 12., theme.ui.claude))
                        })
                        .child(
                            div()
                                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SECONDARY))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.ui.text)
                                .child(comment.author.clone()),
                        ),
                )
                .child(
                    div()
                        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                        .line_height(px(18.))
                        .text_color(theme.ui.text2)
                        .child(comment.body.clone()),
                )
        }))
        .into_any_element()
    }
}

fn pill(text: impl Into<SharedString>, fill: gpui::Hsla, color: gpui::Hsla) -> AnyElement {
    h_flex()
        .flex_none()
        .h(px(16.))
        .px(px(6.))
        .rounded_full()
        .bg(fill)
        .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::SMALL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .whitespace_nowrap()
        .child(text.into())
        .into_any_element()
}

impl Render for CommentsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.presence.is_mounted() {
            return div().into_any_element();
        }
        let theme = Theme::of(cx);
        let grouped = self.grouped(cx);
        let session = self.review.read(cx).session_id();
        let all_views = self.scope == Scope::All;
        let list = if grouped.views.is_empty() {
            let total = grouped.counts.all;
            div()
                .px(px(24.))
                .py(px(40.))
                .text_center()
                .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                .line_height(px(19.))
                .text_color(theme.ui.muted)
                .child(empty_message(self.status, total, self.scope))
                .into_any_element()
        } else {
            v_flex()
                .py(px(8.))
                .children(grouped.views.iter().map(|view| {
                    v_flex()
                        .pb(px(8.))
                        .when(all_views, |this| this.child(self.view_header(&theme, view)))
                        .children(view.files.iter().map(|file| self.file_group(&theme, file, session, cx)))
                }))
                .into_any_element()
        };
        side_sheet("comments", &self.presence)
            .tags("comments:overlay", "comments:panel")
            .width(WIDTH)
            .inset(INSET)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(CommentsEvent::Close);
                }
            }))
            .on_backdrop_click(cx.listener(|_, _, _, cx| cx.emit(CommentsEvent::Close)))
            .child(self.header(&theme, cx))
            .child(self.status_row(&theme, &grouped, cx))
            .child(scroll_y(&self.scroll, div().id("comments-list").child(list)).flex_1())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_list_says_why() {
        let say = |status, total, scope| empty_message(status, total, scope);
        assert!(say(StatusFilter::Open, 0, Scope::View).contains("All views"));
        assert!(say(StatusFilter::Open, 0, Scope::All).contains("repository"));
        assert_eq!(
            say(StatusFilter::Open, 2, Scope::View),
            "No open comments. Everything has been resolved."
        );
        assert_eq!(say(StatusFilter::Resolved, 2, Scope::All), "No resolved comments.");
    }
}
