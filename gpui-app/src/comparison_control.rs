use crate::comparison_menu::{
    Entry, MenuItem as ComparisonItem, Section, base_glyph, base_sections, base_title, branch_glyph, branch_sections,
    branch_title, branch_tooltip,
};
use crate::remote::RemoteMenu;
use crate::tour::{Stop, anchor};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::refs::Refs;
use diffpilot_ui_kit::Theme;
use diffpilot_ui_kit::components::menu::{MenuItem, MenuRow, MenuSection, menu_button};
use diffpilot_ui_kit::components::{Button, CAPSULE_HEIGHT, Look, capsule, tag};
use gpui::prelude::*;
use gpui::{App, IntoElement, ParentElement, Styled, Window, div, px};
use std::rc::Rc;

pub type Select = Rc<dyn Fn(Comparison, &mut Window, &mut App)>;

pub fn comparison_control(
    refs: &Refs,
    comparison: &Comparison,
    compact: bool,
    select: Select,
    remote: RemoteMenu,
    tokens: &Theme,
) -> impl IntoElement {
    let branch_sections = branch_sections(refs, comparison);
    let base_sections = base_sections(refs, comparison);
    let branch_select = select.clone();
    let base_select = select;
    let chip = remote.chip_element(tokens);
    anchor(capsule(tokens, CAPSULE_HEIGHT), Stop::Comparison)
        .px(px(4.))
        .gap(px(4.))
        .child(
            menu_button(
                "comparison-branch",
                tag(
                    segment("comparison-branch", tokens)
                        .icon(branch_glyph(comparison), 16.)
                        .label(branch_title(refs, comparison))
                        .tooltip(branch_tooltip(comparison))
                        .children(chip),
                    || "comparison-branch",
                ),
                move |_, _| {
                    let mut sections = menu_sections(&branch_sections, &branch_select);
                    sections.extend(remote.sections());
                    sections
                },
            )
            .min_width(230.)
            .arrow(),
        )
        .when(!compact, |this| {
            this.child(
                div()
                    .text_size(diffpilot_ui_kit::text_px(diffpilot_ui_kit::text_size::BODY))
                    .text_color(tokens.chrome.secondary)
                    .child("compared to"),
            )
        })
        .child(
            menu_button(
                "comparison-base",
                tag(
                    segment("comparison-base", tokens)
                        .icon(base_glyph(comparison), 16.)
                        .label(base_title(comparison))
                        .tooltip("Compared to"),
                    || "comparison-base",
                ),
                move |_, _| menu_sections(&base_sections, &base_select),
            )
            .min_width(230.)
            .arrow(),
        )
}

fn segment(id: &'static str, tokens: &Theme) -> Button {
    Button::new(id)
        .look(Look::Ghost)
        .regular()
        .h(px(30.))
        .px(px(8.))
        .gap(px(5.))
        .rounded_full()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(tokens.chrome.label)
        .caret(true)
}

fn menu_sections(sections: &[Section], select: &Select) -> Vec<MenuSection> {
    sections
        .iter()
        .map(|section| {
            let rows = section
                .entries
                .iter()
                .map(|entry| match entry {
                    Entry::Item(item) => MenuRow::Item(menu_item(item, select)),
                    Entry::Submenu { title, items } => MenuRow::Submenu {
                        title: (*title).into(),
                        items: items.iter().map(|item| menu_item(item, select)).collect(),
                    },
                })
                .collect();
            MenuSection::titled(section.title, rows).enabled(section.enabled)
        })
        .collect()
}

fn menu_item(item: &ComparisonItem, select: &Select) -> MenuItem {
    let (target, select) = (item.target.clone(), select.clone());
    let row =
        MenuItem::new(item.title.clone(), move |window, cx| select(target.clone(), window, cx)).checked(item.checked);
    match &item.detail {
        Some(detail) => row.detail(detail.clone()),
        None => row,
    }
}
