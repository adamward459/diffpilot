use crate::comparison_menu::{
    Entry, MenuItem, Section, base_glyph, base_sections, base_title, branch_glyph, branch_sections, branch_title,
    branch_tooltip,
};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::refs::Refs;
use diffpilot_ui_kit::Theme;
use diffpilot_ui_kit::components::{Button, Look, capsule};
use gpui::prelude::*;
use gpui::{App, IntoElement, ParentElement, Styled, Window, div, px};
use gpui_component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_component::{ActiveTheme, h_flex};
use std::rc::Rc;

pub type Select = Rc<dyn Fn(Comparison, &mut Window, &mut App)>;

pub fn comparison_control(
    refs: &Refs,
    comparison: &Comparison,
    compact: bool,
    select: Select,
    tokens: &Theme,
) -> impl IntoElement {
    let branch_sections = branch_sections(refs, comparison);
    let base_sections = base_sections(refs, comparison);
    let branch_select = select.clone();
    let base_select = select;
    capsule(tokens, 28.)
        .px(px(4.))
        .gap(px(4.))
        .child(
            segment("comparison-branch", tokens)
                .icon(branch_glyph(comparison), 13.)
                .label(branch_title(refs, comparison))
                .tooltip(branch_tooltip(comparison))
                .dropdown_menu(move |menu, window, cx| fill(menu, &branch_sections, &branch_select, window, cx)),
        )
        .when(!compact, |this| {
            this.child(
                div()
                    .text_size(px(12.))
                    .text_color(tokens.chrome.secondary)
                    .child("compared to"),
            )
        })
        .child(
            segment("comparison-base", tokens)
                .icon(base_glyph(comparison), 13.)
                .label(base_title(comparison))
                .tooltip("Compared to")
                .dropdown_menu(move |menu, window, cx| fill(menu, &base_sections, &base_select, window, cx)),
        )
}

fn segment(id: &'static str, tokens: &Theme) -> Button {
    Button::new(id)
        .look(Look::Ghost)
        .regular()
        .h(px(24.))
        .px(px(8.))
        .gap(px(5.))
        .rounded_full()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(tokens.chrome.label)
        .caret(true)
}

fn fill(
    mut menu: PopupMenu,
    sections: &[Section],
    select: &Select,
    window: &mut Window,
    cx: &mut gpui::Context<PopupMenu>,
) -> PopupMenu {
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            menu = menu.separator();
        }
        menu = menu.label(section.title);
        for entry in &section.entries {
            match entry {
                Entry::Item(item) => menu = menu.item(popup_item(item, section.enabled, select)),
                Entry::Submenu { title, items } => {
                    let items = items.clone();
                    let select = select.clone();
                    menu = menu.submenu(*title, window, cx, move |sub, _, _| {
                        let mut sub = sub.max_h(px(380.)).scrollable(true);
                        for item in &items {
                            sub = sub.item(popup_item(item, true, &select));
                        }
                        sub
                    });
                }
            }
        }
    }
    menu.min_w(px(230.)).max_h(px(520.)).scrollable(true)
}

fn popup_item(item: &MenuItem, enabled: bool, select: &Select) -> PopupMenuItem {
    let target = item.target.clone();
    let select = select.clone();
    let click = move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| select(target.clone(), window, cx);
    match &item.detail {
        None => PopupMenuItem::new(item.title.clone())
            .checked(item.checked)
            .disabled(!enabled)
            .on_click(click),
        Some(detail) => {
            let title = item.title.clone();
            let detail = detail.clone();
            PopupMenuItem::element(move |_, cx| {
                h_flex()
                    .gap_3()
                    .child(title.clone())
                    .child(div().text_color(cx.theme().muted_foreground).child(detail.clone()))
            })
            .checked(item.checked)
            .disabled(!enabled)
            .on_click(click)
        }
    }
}
