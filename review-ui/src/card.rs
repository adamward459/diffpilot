use crate::assets;
use crate::markup::{Markup, SpanKind, pending_mention};
use crate::palette::Palette;
use crate::session::{ReviewEvent, ReviewSession, ThreadChange};
use crate::ui::{Btn, Look, avatar, card_shell, icon_button, severity_badge};
use diffpilot_core::threads::{CLAUDE_NAME, ReviewThread, ThreadAnchor, ThreadComment, ThreadId, mentions_claude};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, EventEmitter, Focusable, FontWeight, Hsla,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Render, SharedString, Styled, StyledText,
    Subscription, TextRun, Window, div, font, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme, Icon, h_flex, v_flex};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Pixel sizes the card lays out with. `ThreadCard::height` adds them up the same way `render` stacks them.
pub mod metrics {
    pub const BORDER: f32 = 1.0;
    pub const FINDING_HEADER: f32 = 38.0;
    pub const PLAIN_HEADER: f32 = 34.0;
    pub const TEXT_SIZE: f32 = 13.0;
    pub const LINE: f32 = 19.0;
    pub const SIDE: f32 = 12.0;
    pub const FINDING_INDENT: f32 = 38.0;
    pub const FINDING_FOOTER: f32 = 32.0;
    pub const SUMMARY_BOTTOM: f32 = 12.0;
    pub const AVATAR: f32 = 22.0;
    pub const AVATAR_GAP: f32 = 10.0;
    pub const NAME_LINE: f32 = 16.0;
    pub const NAME_GAP: f32 = 2.0;
    pub const COMMENT_TOP: f32 = 10.0;
    pub const COMMENT_BOTTOM: f32 = 8.0;
    pub const COMPOSER_TOP: f32 = 8.0;
    pub const COMPOSER_BOTTOM: f32 = 10.0;
    pub const INPUT_ROW: f32 = 18.0;
    pub const INPUT_PAD: f32 = 10.0;
    pub const ACTIONS: f32 = 24.0;
    pub const ACTIONS_GAP: f32 = 8.0;
    pub const SUGGEST: f32 = 26.0;
    pub const MAX_ROWS: usize = 6;
}

use metrics as m;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardEvent {
    Resized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Finding,
    Summary,
    Plain,
}

fn kind_of(thread: &ReviewThread) -> Kind {
    if thread.finding.is_some() {
        Kind::Finding
    } else if matches!(thread.anchor, ThreadAnchor::Change)
        && thread.comments.first().is_some_and(|c| c.author == CLAUDE_NAME)
    {
        Kind::Summary
    } else {
        Kind::Plain
    }
}

struct Composer {
    input: Entity<InputState>,
    rows: usize,
    has_text: bool,
    mention: bool,
    suggest: bool,
    _subscription: Subscription,
}

struct CachedMarkup {
    len: usize,
    streaming: bool,
    markup: Rc<Markup>,
}

pub struct ThreadCard {
    session: Entity<ReviewSession>,
    id: ThreadId,
    collapsed: bool,
    replying: bool,
    composer: Option<Composer>,
    markup: RefCell<HashMap<u32, CachedMarkup>>,
    _session: Subscription,
}

impl EventEmitter<CardEvent> for ThreadCard {}

struct Fonts {
    base: gpui::Font,
    mono: gpui::Font,
    palette: Palette,
}

impl Fonts {
    fn new(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            base: font(theme.font_family.clone()),
            mono: font(theme.mono_font_family.clone()),
            palette: Palette::of(cx),
        }
    }

    fn runs(&self, markup: &Markup) -> Vec<TextRun> {
        let run = |len, font: &gpui::Font, color: Hsla| TextRun {
            len,
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let mut runs = Vec::with_capacity(markup.spans.len() * 2 + 1);
        let mut at = 0;
        for (range, kind) in &markup.spans {
            if range.start > at {
                runs.push(run(range.start - at, &self.base, self.palette.text));
            }
            runs.push(match kind {
                SpanKind::Code => TextRun {
                    background_color: Some(self.palette.text.opacity(0.07)),
                    ..run(range.len(), &self.mono, self.palette.text)
                },
                SpanKind::Mention => {
                    let mut bold = self.base.clone();
                    bold.weight = FontWeight::SEMIBOLD;
                    run(range.len(), &bold, self.palette.claude)
                }
                SpanKind::Caret => run(range.len(), &self.base, self.palette.claude),
            });
            at = range.end;
        }
        if at < markup.text.len() {
            runs.push(run(markup.text.len() - at, &self.base, self.palette.text));
        }
        runs
    }

    fn lines(&self, window: &Window, markup: &Markup, width: Pixels) -> usize {
        if markup.text.is_empty() {
            return 1;
        }
        let runs = self.runs(markup);
        let line = px(m::LINE);
        window
            .text_system()
            .shape_text(markup.text.clone(), px(m::TEXT_SIZE), &runs, Some(width), None)
            .map_or(1, |lines| {
                lines
                    .iter()
                    .map(|l| (l.size(line).height / line).round() as usize)
                    .sum()
            })
    }

    fn text(&self, markup: &Markup) -> AnyElement {
        if markup.text.is_empty() {
            return div().h(px(m::LINE)).into_any_element();
        }
        div()
            .text_size(px(m::TEXT_SIZE))
            .line_height(px(m::LINE))
            .child(StyledText::new(markup.text.clone()).with_runs(self.runs(markup)))
            .into_any_element()
    }
}

impl ThreadCard {
    pub fn new(session: Entity<ReviewSession>, id: ThreadId, cx: &mut Context<Self>) -> Self {
        let nit = session
            .read(cx)
            .thread(id)
            .and_then(|thread| thread.finding.as_ref())
            .is_some_and(|finding| finding.severity == diffpilot_core::review::Severity::Nit);
        let subscription = cx.subscribe(&session, move |_, _, event, cx| match event {
            ReviewEvent::ThreadsChanged(ThreadChange::Updated(changed) | ThreadChange::Removed(changed))
                if *changed == id =>
            {
                cx.notify();
            }
            _ => {}
        });
        Self {
            session,
            id,
            collapsed: nit,
            replying: false,
            composer: None,
            markup: RefCell::default(),
            _session: subscription,
        }
    }

    pub fn thread_id(&self) -> ThreadId {
        self.id
    }

    pub fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if kind_of_thread(self, cx).is_some_and(|kind| kind != Kind::Plain) {
            self.replying = true;
        }
        self.ensure_composer(window, cx);
        if let Some(composer) = &self.composer {
            composer.input.update(cx, |input, cx| input.focus(window, cx));
        }
        cx.emit(CardEvent::Resized);
        cx.notify();
    }

    fn markup_of(&self, comment: &ThreadComment) -> Rc<Markup> {
        let mut cache = self.markup.borrow_mut();
        if let Some(cached) = cache.get(&comment.id)
            && cached.len == comment.body.len()
            && cached.streaming == comment.is_streaming
        {
            return cached.markup.clone();
        }
        let markup = Rc::new(display_markup(comment));
        cache.insert(
            comment.id,
            CachedMarkup {
                len: comment.body.len(),
                streaming: comment.is_streaming,
                markup: markup.clone(),
            },
        );
        markup
    }

    fn shows_composer(&self, kind: Kind) -> bool {
        kind == Kind::Plain || self.replying
    }

    /// Same until the thread changes, a card toggle flips, or the composer grows. Mirrors `render`'s layout.
    pub fn height(&self, width: Pixels, window: &Window, cx: &App) -> Pixels {
        let session = self.session.read(cx);
        let Some(thread) = session.thread(self.id) else {
            return px(0.);
        };
        let fonts = Fonts::new(cx);
        let kind = kind_of(thread);
        let inner = f32::from(width) - 2.0 * m::BORDER;
        let text_lines = |comment: &ThreadComment, wrap: f32| {
            fonts.lines(window, &self.markup_of(comment), px(wrap.max(1.0))) as f32 * m::LINE
        };
        let side = 2.0 * m::SIDE;
        let comment_wrap = inner - side - m::AVATAR - m::AVATAR_GAP;
        let comment_height = |comment: &ThreadComment, first: bool| {
            let border = if first { 0.0 } else { m::BORDER };
            border + m::COMMENT_TOP + m::NAME_LINE + m::NAME_GAP + text_lines(comment, comment_wrap) + m::COMMENT_BOTTOM
        };
        let first = thread.comments.first();
        let mut total = 2.0 * m::BORDER;
        match kind {
            Kind::Finding => {
                total += m::FINDING_HEADER;
                if !self.collapsed {
                    let wrap = inner - m::FINDING_INDENT - m::SIDE;
                    total += first.map_or(m::LINE, |c| text_lines(c, wrap)) + m::FINDING_FOOTER;
                    total += thread
                        .comments
                        .iter()
                        .skip(1)
                        .map(|c| comment_height(c, false))
                        .sum::<f32>();
                    if self.shows_composer(kind) {
                        total += self.composer_height(thread, cx);
                    }
                }
            }
            Kind::Summary => {
                total += m::FINDING_HEADER;
                total += first.map_or(m::LINE, |c| text_lines(c, inner - side)) + m::SUMMARY_BOTTOM;
                total += thread
                    .comments
                    .iter()
                    .skip(1)
                    .map(|c| comment_height(c, false))
                    .sum::<f32>();
                if self.shows_composer(kind) {
                    total += self.composer_height(thread, cx);
                }
            }
            Kind::Plain => {
                total += m::PLAIN_HEADER;
                total += thread
                    .comments
                    .iter()
                    .enumerate()
                    .map(|(index, c)| comment_height(c, index == 0))
                    .sum::<f32>();
                total += self.composer_height(thread, cx);
            }
        }
        px(total)
    }

    fn composer_height(&self, thread: &ReviewThread, _cx: &App) -> f32 {
        let (rows, actions, suggest) = match &self.composer {
            Some(c) => (c.rows, c.has_text || thread.comments.is_empty(), c.suggest),
            None => (1, thread.comments.is_empty(), false),
        };
        let input = rows as f32 * m::INPUT_ROW + m::INPUT_PAD;
        m::BORDER
            + m::COMPOSER_TOP
            + input
            + if suggest { m::SUGGEST } else { 0.0 }
            + if actions { m::ACTIONS_GAP + m::ACTIONS } else { 0.0 }
            + m::COMPOSER_BOTTOM
    }

    fn ensure_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer.is_some() {
            return;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, m::MAX_ROWS)
                .placeholder(self.placeholder(cx))
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.input_changed(input, cx),
                InputEvent::PressEnter { secondary: true } => this.submit(window, cx),
                _ => {}
            },
        );
        self.composer = Some(Composer {
            input,
            rows: 1,
            has_text: false,
            mention: false,
            suggest: false,
            _subscription: subscription,
        });
    }

    fn placeholder(&self, cx: &App) -> SharedString {
        let replying = self
            .session
            .read(cx)
            .thread(self.id)
            .is_some_and(|t| !t.comments.is_empty());
        if replying {
            "Reply… type @claude to ask Claude"
        } else {
            "Leave a comment… type @claude to ask Claude"
        }
        .into()
    }

    fn input_changed(&mut self, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let value = input.read(cx).value();
        let Some(composer) = &mut self.composer else { return };
        let rows = value.split('\n').count().clamp(1, m::MAX_ROWS);
        let has_text = !value.trim().is_empty();
        let resized = rows != composer.rows || has_text != composer.has_text;
        composer.rows = rows;
        composer.has_text = has_text;
        composer.mention = mentions_claude(&value);
        composer.suggest = pending_mention(&value).is_some();
        if resized {
            cx.emit(CardEvent::Resized);
        }
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = &self.composer else { return };
        let text = composer.input.read(cx).value();
        if text.trim().is_empty() {
            return;
        }
        let id = self.id;
        self.session.update(cx, |session, cx| session.post(id, &text, cx));
        composer.input.update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn complete_mention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = &self.composer else { return };
        let value = composer.input.read(cx).value();
        let Some(typed) = pending_mention(&value) else { return };
        let rest = format!("{} ", &"claude"[typed.len()..]);
        composer.input.update(cx, |input, cx| input.insert(rest, window, cx));
    }

    fn resolve(&mut self, cx: &mut Context<Self>) {
        let id = self.id;
        self.session.update(cx, |session, cx| session.remove_thread(id, cx));
    }

    fn toggle_collapsed(&mut self, cx: &mut Context<Self>) {
        self.collapsed = !self.collapsed;
        cx.emit(CardEvent::Resized);
        cx.notify();
    }

    fn open_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.replying = true;
        self.ensure_composer(window, cx);
        if let Some(composer) = &self.composer {
            composer.input.update(cx, |input, cx| input.focus(window, cx));
        }
        cx.emit(CardEvent::Resized);
        cx.notify();
    }

    fn release_idle_composer(&mut self, window: &Window, cx: &App) {
        let Some(composer) = &self.composer else { return };
        let is_draft = self
            .session
            .read(cx)
            .thread(self.id)
            .is_none_or(|t| t.comments.is_empty());
        let input = composer.input.read(cx);
        if !is_draft && !input.focus_handle(cx).is_focused(window) && input.value().is_empty() {
            self.composer = None;
        }
    }

    fn button(&self, label: &'static str, key: &'static str) -> Btn {
        Btn::new((key, self.id.0 as usize)).label(label)
    }

    fn ask_button(&self, busy: bool) -> Btn {
        self.button("Ask Claude", "ask")
            .look(Look::Claude)
            .icon(assets::SPARKLES, 12.)
            .disabled(busy)
    }

    fn comment(&self, comment: &ThreadComment, first: bool, fonts: &Fonts) -> AnyElement {
        let palette = fonts.palette;
        h_flex()
            .items_start()
            .gap(px(m::AVATAR_GAP))
            .px(px(m::SIDE))
            .pt(px(m::COMMENT_TOP))
            .pb(px(m::COMMENT_BOTTOM))
            .when(!first, |row| row.border_t_1().border_color(palette.line))
            .child(avatar(
                &comment.author,
                comment.author == CLAUDE_NAME,
                palette,
                m::AVATAR,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .h(px(m::NAME_LINE))
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(comment.author.clone()),
                    )
                    .child(div().mt(px(m::NAME_GAP)).child(fonts.text(&self.markup_of(comment)))),
            )
            .into_any_element()
    }

    fn composer_view(
        &self,
        thread: &ReviewThread,
        palette: Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let draft = thread.comments.is_empty();
        let (rows, has_text, mention, suggest) = self
            .composer
            .as_ref()
            .map_or((1, false, false, false), |c| (c.rows, c.has_text, c.mention, c.suggest));
        let _ = window;
        let field = match &self.composer {
            Some(composer) => div()
                .min_h(px(rows as f32 * m::INPUT_ROW + m::INPUT_PAD))
                .child(
                    Input::new(&composer.input)
                        .appearance(true)
                        .text_size(px(12.5))
                        .line_height(px(m::INPUT_ROW))
                        .px(px(9.))
                        .py(px(4.)),
                )
                .into_any_element(),
            None => div()
                .id(("composer", self.id.0 as usize))
                .h(px(m::INPUT_ROW + m::INPUT_PAD))
                .px(px(9.))
                .flex()
                .items_center()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(palette.line)
                .bg(palette.field)
                .text_size(px(12.5))
                .text_color(palette.muted)
                .cursor_text()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.focus_composer(window, cx)),
                )
                .child(self.placeholder(cx))
                .into_any_element(),
        };
        let id = self.id.0 as usize;
        v_flex()
            .px(px(m::SIDE))
            .pt(px(m::COMPOSER_TOP))
            .pb(px(m::COMPOSER_BOTTOM))
            .border_t_1()
            .border_color(palette.line)
            .child(field)
            .when(suggest, |col| {
                col.child(
                    h_flex()
                        .id(("suggest", id))
                        .h(px(m::SUGGEST))
                        .gap_2()
                        .items_center()
                        .text_size(px(12.))
                        .text_color(palette.claude)
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| this.complete_mention(window, cx)),
                        )
                        .child(Icon::empty().path(assets::SPARKLES).size_3())
                        .child("@claude")
                        .child(div().text_color(palette.muted).child("Ask Claude in this thread")),
                )
            })
            .when(has_text || draft, |col| {
                col.child(
                    h_flex()
                        .mt(px(m::ACTIONS_GAP))
                        .h(px(m::ACTIONS))
                        .gap_2()
                        .items_center()
                        .when(mention, |row| {
                            row.child(
                                h_flex()
                                    .gap_1()
                                    .px_2()
                                    .h(px(20.))
                                    .items_center()
                                    .rounded_full()
                                    .bg(palette.claude_bg)
                                    .text_color(palette.claude)
                                    .text_size(px(11.))
                                    .child(Icon::empty().path(assets::SPARKLES).size(px(11.)))
                                    .child("Claude will reply"),
                            )
                        })
                        .child(div().flex_1())
                        .when(draft, |row| {
                            row.child(
                                self.button("Cancel", "cancel")
                                    .on_click(cx.listener(|this, _, _, cx| this.resolve(cx))),
                            )
                        })
                        .child(
                            self.button(if draft { "Comment" } else { "Reply" }, "post")
                                .look(Look::Primary)
                                .hint("⌘↩")
                                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                        ),
                )
            })
            .into_any_element()
    }

    fn finding_card(&self, thread: &ReviewThread, fonts: &Fonts, window: &Window, cx: &Context<Self>) -> AnyElement {
        let palette = fonts.palette;
        let Some(finding) = &thread.finding else {
            return div().into_any_element();
        };
        let color = palette.severity(finding.severity);
        let place = match &thread.anchor {
            ThreadAnchor::Line { line, .. } => Some(format!("Line {line}")),
            _ => finding.location.clone(),
        };
        let border = if finding.severity == diffpilot_core::review::Severity::Nit {
            palette.line
        } else {
            color.opacity(0.45)
        };
        let collapsed = self.collapsed;
        let header = h_flex()
            .h(px(m::FINDING_HEADER))
            .pl(px(m::SIDE))
            .pr_2()
            .gap_2()
            .items_center()
            .text_size(px(m::TEXT_SIZE))
            .child(severity_badge(finding.severity, palette, 15.))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(finding.title.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color)
                    .child(finding.severity.title()),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .when(!collapsed, |row| {
                        row.child(
                            self.button("Reply", "reply")
                                .on_click(cx.listener(|this, _, window, cx| this.open_reply(window, cx))),
                        )
                        .child(
                            self.button("Resolve", "resolve")
                                .on_click(cx.listener(|this, _, _, cx| this.resolve(cx))),
                        )
                    })
                    .child(
                        icon_button(
                            ("fold", self.id.0 as usize),
                            if collapsed {
                                assets::CHEVRON_DOWN
                            } else {
                                assets::CHEVRON_UP
                            },
                            palette.muted,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_collapsed(cx))),
                    ),
            );
        let body = (!collapsed).then(|| {
            let first = thread.comments.first();
            v_flex()
                .child(div().pl(px(m::FINDING_INDENT)).pr(px(m::SIDE)).child(first.map_or_else(
                    || div().h(px(m::LINE)).into_any_element(),
                    |c| fonts.text(&self.markup_of(c)),
                )))
                .child(
                    h_flex()
                        .h(px(m::FINDING_FOOTER))
                        .pl(px(m::FINDING_INDENT))
                        .pt(px(6.))
                        .gap_1p5()
                        .items_start()
                        .text_size(px(11.5))
                        .text_color(palette.muted)
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .text_color(palette.claude)
                                .child(Icon::empty().path(assets::SPARKLES).size(px(11.)))
                                .child(CLAUDE_NAME),
                        )
                        .children(place.map(|place| format!("·  {place}"))),
                )
                .children(thread.comments.iter().skip(1).map(|c| self.comment(c, false, fonts)))
                .when(self.replying, |col| {
                    col.child(self.composer_view(thread, palette, window, cx))
                })
        });
        card_shell(palette, border, cx.theme().is_dark())
            .child(header)
            .children(body)
            .into_any_element()
    }

    fn summary_card(&self, thread: &ReviewThread, fonts: &Fonts, window: &Window, cx: &Context<Self>) -> AnyElement {
        let palette = fonts.palette;
        let first = thread.comments.first();
        let copy_text = first.map(|c| c.body.clone()).unwrap_or_default();
        let header = h_flex()
            .h(px(m::FINDING_HEADER))
            .pl(px(m::SIDE))
            .pr_2()
            .gap_2()
            .items_center()
            .text_size(px(m::TEXT_SIZE))
            .child(
                div()
                    .text_color(palette.claude)
                    .child(Icon::empty().path(assets::SPARKLES).size(px(13.))),
            )
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Review summary"))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(palette.muted)
                    .child("Whole change · Claude"),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        self.button("Copy as Markdown", "copy")
                            .icon(assets::COPY, 11.)
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()))
                            }),
                    )
                    .child(
                        self.button("Reply", "reply")
                            .on_click(cx.listener(|this, _, window, cx| this.open_reply(window, cx))),
                    )
                    .child(
                        self.button("Resolve", "resolve")
                            .on_click(cx.listener(|this, _, _, cx| this.resolve(cx))),
                    ),
            );
        card_shell(palette, palette.claude_border, cx.theme().is_dark())
            .child(header)
            .child(div().px(px(m::SIDE)).pb(px(m::SUMMARY_BOTTOM)).child(first.map_or_else(
                || div().h(px(m::LINE)).into_any_element(),
                |c| fonts.text(&self.markup_of(c)),
            )))
            .children(thread.comments.iter().skip(1).map(|c| self.comment(c, false, fonts)))
            .when(self.replying, |col| {
                col.child(self.composer_view(thread, palette, window, cx))
            })
            .into_any_element()
    }

    fn plain_card(&self, thread: &ReviewThread, fonts: &Fonts, window: &Window, cx: &Context<Self>) -> AnyElement {
        let palette = fonts.palette;
        let busy = thread.comments.iter().any(|c| c.is_streaming);
        let claude = thread.comments.iter().any(|c| c.author == CLAUDE_NAME);
        let label = match &thread.anchor {
            ThreadAnchor::Change => "Review summary".to_string(),
            ThreadAnchor::File { .. } => "File comment".to_string(),
            ThreadAnchor::Line { line, side, .. } => {
                let old = if matches!(side, diffpilot_core::threads::Side::Old) {
                    " (old)"
                } else {
                    ""
                };
                format!("Line {line}{old}")
            }
        };
        let id = self.id;
        let header = h_flex()
            .h(px(m::PLAIN_HEADER))
            .pl(px(m::SIDE))
            .pr_2()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(palette.line)
            .text_size(px(12.))
            .text_color(palette.muted)
            .child(label)
            .when(thread.comments.is_empty(), |row| {
                row.child(
                    div()
                        .px_2()
                        .h(px(18.))
                        .flex()
                        .items_center()
                        .rounded_full()
                        .bg(palette.amber.opacity(0.16))
                        .text_color(palette.amber)
                        .text_size(px(11.))
                        .child("Draft"),
                )
            })
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_1()
                    .child(self.ask_button(busy).on_click(cx.listener(move |this, _, _, cx| {
                        this.session.update(cx, |session, cx| session.ask_claude(id, cx));
                    })))
                    .when(!thread.comments.is_empty(), |row| {
                        row.child(
                            self.button("Resolve", "resolve")
                                .on_click(cx.listener(|this, _, _, cx| this.resolve(cx))),
                        )
                    }),
            );
        card_shell(
            palette,
            if claude { palette.claude_border } else { palette.line },
            cx.theme().is_dark(),
        )
        .child(header)
        .children(
            thread
                .comments
                .iter()
                .enumerate()
                .map(|(index, c)| self.comment(c, index == 0, fonts)),
        )
        .child(self.composer_view(thread, palette, window, cx))
        .into_any_element()
    }
}

fn kind_of_thread(card: &ThreadCard, cx: &App) -> Option<Kind> {
    card.session.read(cx).thread(card.id).map(kind_of)
}

fn display_markup(comment: &ThreadComment) -> Markup {
    let mut markup = Markup::parse(&comment.body);
    if !comment.is_streaming {
        return markup;
    }
    let mut text = String::from(markup.text.as_ref());
    let start = text.len();
    text.push_str(if comment.body.is_empty() { "···" } else { "▍" });
    markup.spans.push((start..text.len(), SpanKind::Caret));
    markup.text = text.into();
    markup
}

impl Render for ThreadCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draft = self
            .session
            .read(cx)
            .thread(self.id)
            .is_some_and(|thread| thread.comments.is_empty());
        if draft && self.composer.is_none() {
            self.ensure_composer(window, cx);
            if let Some(composer) = &self.composer {
                let input = composer.input.clone();
                window.defer(cx, move |window, cx| {
                    input.update(cx, |input, cx| input.focus(window, cx))
                });
            }
        }
        self.release_idle_composer(window, cx);
        let session = self.session.clone();
        let session = session.read(cx);
        let Some(thread) = session.thread(self.id) else {
            return div().into_any_element();
        };
        let fonts = Fonts::new(cx);
        let card = match kind_of(thread) {
            Kind::Finding => self.finding_card(thread, &fonts, window, cx),
            Kind::Summary => self.summary_card(thread, &fonts, window, cx),
            Kind::Plain => self.plain_card(thread, &fonts, window, cx),
        };
        div()
            .w_full()
            .font_family(cx.theme().font_family.clone())
            .text_size(px(m::TEXT_SIZE))
            .child(card)
            .into_any_element()
    }
}
