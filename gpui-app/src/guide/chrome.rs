use crate::format::ago;
use crate::guide::navigator::now_ms;
use crate::guide::session::{GuideRun, GuideSession, Step};
use diffpilot_core::file_kind::file_kind;
use diffpilot_core::guide::reading::ChapterKey;
use diffpilot_ui_kit::components::{Button, Crumb, Look, bar_button, crumbs, jump_bar, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::{App, Entity, InteractiveElement as _, ParentElement as _, SharedString, Stateful, Styled as _, div, px};
use gpui_component::h_flex;

fn guide_crumb(tokens: &Theme, last: bool) -> Crumb {
    Crumb {
        glyph: Glyph::BookOpen,
        color: Some(tokens.ui.claude),
        label: "Guide".into(),
        last,
    }
}

pub fn overview_bar(guide: &Entity<GuideSession>, tokens: &Theme, cx: &App) -> Stateful<gpui::Div> {
    let session = guide.read(cx);
    let writing = matches!(session.run(), GuideRun::Writing { .. });
    let mut bar = jump_bar(tokens).id("guide-jump-bar");
    let Some(stored) = session.stored().filter(|_| !writing) else {
        return bar
            .child(crumbs(tokens, vec![guide_crumb(tokens, true)]))
            .child(div().flex_1());
    };
    let model = stored
        .model
        .map(|model| format!(" \u{b7} {}", model.title()))
        .unwrap_or_default();
    let written = format!(
        "Written by Claude \u{b7} {}{model}",
        ago(now_ms(), stored.finished_at_ms.unwrap_or(stored.created_at_ms))
    );
    let (rewrite, start) = (guide.clone(), guide.clone());
    let started = session.has_progress();
    let empty = session.reading().chapters.is_empty();
    bar = bar
        .child(crumbs(
            tokens,
            vec![
                guide_crumb(tokens, false),
                Crumb {
                    glyph: Glyph::FileText,
                    color: None,
                    label: "Overview".into(),
                    last: true,
                },
            ],
        ))
        .child(div().flex_1())
        .child(
            div()
                .text_size(px(12.))
                .text_color(tokens.ui.muted)
                .child(SharedString::from(written)),
        )
        .child(tag(
            Button::new("guide-rewrite")
                .icon(Glyph::Sparkles, 12.)
                .label("Rewrite")
                .on_click(move |_, _, cx| rewrite.update(cx, |guide, cx| guide.write(cx))),
            || "guide-rewrite",
        ));
    if !empty {
        bar = bar.child(tag(
            Button::new("guide-start-reading")
                .look(Look::Primary)
                .icon(Glyph::ChevronRight, 11.)
                .label(if started { "Continue" } else { "Start Reading" })
                .on_click(move |_, _, cx| start.update(cx, |guide, cx| guide.start_reading(cx))),
            || "guide-start-reading",
        ));
    }
    bar
}

pub fn chapter_bar(
    guide: &Entity<GuideSession>,
    visible_file: Option<&str>,
    tokens: &Theme,
    cx: &App,
) -> Stateful<gpui::Div> {
    let session = guide.read(cx);
    let mut items = vec![guide_crumb(tokens, false)];
    let mut position = String::new();
    if let (Some(chapter), Step::Chapter(index)) = (session.chapter(), session.step()) {
        items.push(Crumb {
            glyph: Glyph::File,
            color: None,
            label: chapter.title.clone().into(),
            last: visible_file.is_none(),
        });
        if chapter.key != ChapterKey::Extra {
            position = format!("{} / {}", index + 1, session.reading().guided());
        }
    }
    if let Some(name) = visible_file.map(|path| path.rsplit('/').next().unwrap_or(path).to_string()) {
        items.push(Crumb {
            glyph: Glyph::for_file(&name),
            color: Some(tokens.tint(file_kind(&name).tint())),
            label: name.into(),
            last: true,
        });
    }
    let (previous, next) = (guide.clone(), guide.clone());
    jump_bar(tokens)
        .id("guide-jump-bar")
        .child(div().flex_1().min_w_0().child(crumbs(tokens, items)))
        .child(
            h_flex()
                .gap(px(2.))
                .flex_none()
                .text_size(px(13.))
                .text_color(tokens.ui.text2)
                .child(bar_button(
                    tokens,
                    "guide-bar-previous",
                    Glyph::ChevronLeft,
                    "Previous chapter",
                    true,
                    false,
                    move |_, _, cx| previous.update(cx, |guide, cx| guide.previous(cx)),
                ))
                .child(div().id("guide-stepper").child(SharedString::from(position)))
                .child(bar_button(
                    tokens,
                    "guide-bar-next",
                    Glyph::ChevronRight,
                    "Next chapter",
                    true,
                    false,
                    move |_, _, cx| next.update(cx, |guide, cx| guide.next(cx)),
                )),
        )
        .child(div().w(px(4.)))
}
