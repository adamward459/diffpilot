use crate::guide::text::{CodeRef, Inline, InlineKind};
use diffpilot_ui_kit::Theme;
use gpui::{
    AnyElement, App, ElementId, FontWeight, Hsla, InteractiveText, IntoElement, Pixels, StyledText, TextRun, Window,
    font, px,
};
use gpui_component::ActiveTheme;
use std::rc::Rc;

pub type OnRef = Rc<dyn Fn(&CodeRef, &mut Window, &mut App)>;

#[derive(Clone, Copy)]
pub struct Body {
    pub size: f32,
    pub line: f32,
    pub color: Hsla,
}

impl Body {
    pub fn new(theme: &Theme, size: f32, line: f32) -> Self {
        Self {
            size,
            line,
            color: theme.ui.text,
        }
    }

    pub fn colored(self, color: Hsla) -> Self {
        Self { color, ..self }
    }
}

fn runs(inline: &Inline, body: Body, cx: &App) -> Vec<TextRun> {
    let theme = Theme::of(cx);
    let base = font(cx.theme().font_family.clone());
    let mono = font(cx.theme().mono_font_family.clone());
    let run = |len: usize, font: &gpui::Font, color: Hsla| TextRun {
        len,
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut out = Vec::with_capacity(inline.spans.len() * 2 + 1);
    let mut at = 0;
    for (range, kind) in &inline.spans {
        if range.start > at {
            out.push(run(range.start - at, &base, body.color));
        }
        out.push(match kind {
            InlineKind::Code => TextRun {
                background_color: Some(body.color.opacity(0.07)),
                ..run(range.len(), &mono, body.color)
            },
            InlineKind::Ref(_) => {
                let mut bold = base.clone();
                bold.weight = FontWeight::SEMIBOLD;
                TextRun {
                    background_color: Some(theme.ui.claude_bg),
                    ..run(range.len(), &bold, theme.ui.claude)
                }
            }
        });
        at = range.end;
    }
    if at < inline.text.len() {
        out.push(run(inline.text.len() - at, &base, body.color));
    }
    out
}

pub fn line_count(inline: &Inline, body: Body, width: Pixels, window: &Window, cx: &App) -> usize {
    if inline.text.is_empty() {
        return 1;
    }
    let line = px(body.line);
    window
        .text_system()
        .shape_text(
            inline.text.clone(),
            px(body.size),
            &runs(inline, body, cx),
            Some(width),
            None,
        )
        .map_or(1, |lines| {
            lines
                .iter()
                .map(|l| (l.size(line).height / line).round() as usize)
                .sum()
        })
}

pub fn element(id: impl Into<ElementId>, inline: &Inline, body: Body, on_ref: Option<OnRef>, cx: &App) -> AnyElement {
    use gpui::{ParentElement as _, Styled as _, div};
    let styled = StyledText::new(inline.text.clone()).with_runs(runs(inline, body, cx));
    let text = match on_ref {
        Some(on_ref) if inline.refs().next().is_some() => {
            let (ranges, targets): (Vec<_>, Vec<_>) =
                inline.refs().map(|(range, code)| (range.clone(), code.clone())).unzip();
            InteractiveText::new(id, styled)
                .on_click(ranges, move |index, window, cx| on_ref(&targets[index], window, cx))
                .into_any_element()
        }
        _ => styled.into_any_element(),
    };
    div()
        .text_size(px(body.size))
        .line_height(px(body.line))
        .child(text)
        .into_any_element()
}
