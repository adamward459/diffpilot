use crate::comparison_menu::{Entry, MenuItem as ComparisonItem, Section, base_title, branch_glyph, branch_title};
use crate::remote::RemoteMenu;
use crate::tour::{Stop, anchor};
use crate::workspace::ChangeComparison;
use diffpilot_core::comparison::Comparison;
use diffpilot_core::refs::Refs;
use diffpilot_ui_kit::Theme;
use diffpilot_ui_kit::components::menu::{MenuItem, MenuRow, MenuSection};
use diffpilot_ui_kit::components::{Button, CAPSULE_HEIGHT, Look, capsule, tag};
use gpui::prelude::*;
use gpui::{App, IntoElement, ParentElement, Styled, Window, div, px};
use std::rc::Rc;

pub type Select = Rc<dyn Fn(Comparison, &mut Window, &mut App)>;

pub fn comparison_control(
    refs: &Refs,
    comparison: &Comparison,
    compact: bool,
    remote: RemoteMenu,
    tokens: &Theme,
) -> impl IntoElement {
    let chip = remote.chip_element(tokens);
    let label = format!("{} \u{2192} {}", branch_title(refs, comparison), base_title(comparison));
    let button = Button::new("comparison-label")
        .look(Look::Ghost)
        .regular()
        .h(px(30.))
        .px(px(8.))
        .gap(px(5.))
        .rounded_full()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(tokens.chrome.label)
        .when(!compact, |button| button.icon(branch_glyph(comparison), 16.))
        .label(label.clone())
        .tooltip("Change what you compare")
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ChangeComparison), cx));
    anchor(capsule(tokens, CAPSULE_HEIGHT), Stop::Comparison)
        .px(px(4.))
        .gap(px(4.))
        .child(tag(div().flex().child(tag(button, || "comparison-label")), move || {
            format!("comparison-text:{label}")
        }))
        .children(chip)
}

pub(crate) fn menu_sections(sections: &[Section], select: &Select) -> Vec<MenuSection> {
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
