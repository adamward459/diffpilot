use super::model::{SlotKind, jump, layout, step};
use super::*;
use gpui::prelude::*;
use gpui::{Context, Entity, Modifiers, Render, TestAppContext, VisualTestContext, Window, div, point, px, size};
use std::cell::RefCell;
use std::rc::Rc;

fn item(label: &str) -> MenuItem {
    MenuItem::new(label.to_string(), |_, _| {})
}

fn rows(labels: &[&str]) -> Vec<MenuRow> {
    labels.iter().map(|label| item(label).into()).collect()
}

#[test]
fn the_layout_skips_empty_sections_and_puts_a_separator_between_the_rest() {
    let sections = vec![
        MenuSection::titled("A", rows(&["one"])),
        MenuSection::titled("Empty", vec![]),
        MenuSection::new(rows(&["two", "three"])).enabled(false),
    ];
    let kinds: Vec<SlotKind> = layout(&sections).iter().map(|slot| slot.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SlotKind::Title { section: 0 },
            SlotKind::Row {
                section: 0,
                row: 0,
                enabled: true
            },
            SlotKind::Separator,
            SlotKind::Row {
                section: 2,
                row: 0,
                enabled: false
            },
            SlotKind::Row {
                section: 2,
                row: 1,
                enabled: false
            },
        ]
    );
}

#[test]
fn arrow_keys_skip_disabled_rows_and_wrap_around() {
    let sections = vec![
        MenuSection::new(rows(&["a"])),
        MenuSection::new(rows(&["off"])).enabled(false),
        MenuSection::new(rows(&["b"])),
    ];
    let slots = layout(&sections);
    let first = step(&slots, None, true);
    assert_eq!(first, Some(0));
    let second = step(&slots, first, true);
    assert_eq!(second, Some(4));
    assert_eq!(step(&slots, second, true), Some(0));
    assert_eq!(step(&slots, None, false), Some(4));
}

#[test]
fn typing_a_letter_jumps_to_the_next_row_that_starts_with_it() {
    let sections = vec![MenuSection::new(rows(&["main", "Develop", "master"]))];
    let slots = layout(&sections);
    assert_eq!(jump(&slots, &sections, None, 'm'), Some(0));
    assert_eq!(jump(&slots, &sections, Some(0), 'm'), Some(2));
    assert_eq!(jump(&slots, &sections, Some(2), 'm'), Some(0));
    assert_eq!(jump(&slots, &sections, None, 'd'), Some(1));
    assert_eq!(jump(&slots, &sections, None, 'x'), None);
}

struct Harness {
    picked: Rc<RefCell<Vec<String>>>,
    arrow: bool,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let picked = self.picked.clone();
        let pick = move |label: String| {
            let picked = picked.clone();
            MenuItem::new(label.clone(), move |_, _| picked.borrow_mut().push(label.clone()))
        };
        let trigger = div()
            .id("open")
            .debug_selector(|| "open".into())
            .w(px(80.))
            .h(px(24.))
            .child("Open");
        let arrow = self.arrow;
        div().size_full().p(px(20.)).child(
            menu_button("m", trigger, move |_, _| {
                let branches = (0..5).map(|ix| pick(format!("b{ix}"))).collect();
                let long = (0..40).map(|ix| pick(format!("Item {ix}")).into()).collect();
                vec![
                    MenuSection::titled(
                        "Refs",
                        vec![MenuRow::Submenu {
                            title: "Branches".into(),
                            items: branches,
                        }],
                    ),
                    MenuSection::titled("Disabled", vec![pick("Off".into()).into()]).enabled(false),
                    MenuSection::new(long),
                ]
            })
            .min_width(200.)
            .when(arrow, MenuButton::arrow),
        )
    }
}

struct Rig {
    root: Entity<gpui_component::Root>,
    picked: Rc<RefCell<Vec<String>>>,
}

fn rig(cx: &mut TestAppContext) -> (Rig, &mut VisualTestContext) {
    rig_with(cx, false)
}

fn rig_with(cx: &mut TestAppContext, arrow: bool) -> (Rig, &mut VisualTestContext) {
    cx.update(crate::init);
    let picked = Rc::new(RefCell::new(Vec::new()));
    let handle = cx.add_window(|window, cx| {
        let harness = cx.new(|_| Harness {
            picked: picked.clone(),
            arrow,
        });
        gpui_component::Root::new(harness, window, cx)
    });
    let root = handle.root(cx).unwrap();
    let vcx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    let rig = Rig { root, picked };
    rig.settle(vcx);
    (rig, vcx)
}

impl Rig {
    fn settle(&self, cx: &mut VisualTestContext) {
        let root = self.root.clone();
        cx.run_until_parked();
        cx.draw(point(px(0.), px(0.)), size(px(800.), px(700.)), move |_, _| root);
        cx.run_until_parked();
    }

    fn center(&self, cx: &mut VisualTestContext, selector: &'static str) -> gpui::Point<gpui::Pixels> {
        self.settle(cx);
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("no element tagged {selector}"))
            .center()
    }

    fn click(&self, cx: &mut VisualTestContext, selector: &'static str) {
        let at = self.center(cx, selector);
        cx.simulate_click(at, Modifiers::default());
        self.settle(cx);
    }

    fn hover(&self, cx: &mut VisualTestContext, selector: &'static str) {
        let at = self.center(cx, selector);
        cx.simulate_mouse_move(at, None, Modifiers::default());
        self.settle(cx);
    }

    fn keys(&self, cx: &mut VisualTestContext, keys: &str) {
        cx.simulate_keystrokes(keys);
        self.settle(cx);
    }

    fn picked(&self) -> Vec<String> {
        self.picked.borrow().clone()
    }
}

// gpui 0.2.2 never clears `debug_bounds`, so a closed menu still has bounds. These tests click where
// a row was and check what got picked.

#[gpui::test]
async fn a_submenu_of_a_scrolling_menu_can_be_clicked(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.click(cx, "open");
    rig.hover(cx, "m:Branches");
    rig.click(cx, "m:Branches:b2");
    assert_eq!(rig.picked(), vec!["b2"]);
    rig.click(cx, "m:Item 0");
    assert_eq!(rig.picked(), vec!["b2"], "picking a row closes the whole menu");
}

#[gpui::test]
async fn hovering_a_plain_row_closes_the_open_submenu(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.click(cx, "open");
    rig.hover(cx, "m:Branches");
    let b0 = rig.center(cx, "m:Branches:b0");
    rig.hover(cx, "m:Item 0");
    cx.simulate_click(b0, Modifiers::default());
    rig.settle(cx);
    assert!(rig.picked().is_empty());
}

#[gpui::test]
async fn the_keyboard_moves_past_disabled_rows_and_picks(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.click(cx, "open");
    rig.keys(cx, "down down enter");
    assert_eq!(rig.picked(), vec!["Item 0"]);
    rig.click(cx, "m:Item 1");
    assert_eq!(rig.picked(), vec!["Item 0"]);
}

#[gpui::test]
async fn right_and_left_open_and_close_a_submenu(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.click(cx, "open");
    rig.keys(cx, "down right down enter");
    assert_eq!(rig.picked(), vec!["b1"]);

    rig.click(cx, "open");
    rig.keys(cx, "down right left down enter");
    assert_eq!(rig.picked(), vec!["b1", "Item 0"]);
}

#[gpui::test]
async fn escape_and_a_click_outside_close_without_picking(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    rig.click(cx, "open");
    rig.keys(cx, "escape");
    rig.click(cx, "m:Item 0");

    rig.click(cx, "open");
    cx.simulate_click(point(px(700.), px(650.)), Modifiers::default());
    rig.settle(cx);
    rig.click(cx, "m:Item 0");
    assert!(rig.picked().is_empty());
}

#[gpui::test]
async fn a_menu_with_an_arrow_opens_and_picks_a_row(cx: &mut TestAppContext) {
    let (rig, cx) = rig_with(cx, true);
    rig.click(cx, "open");
    rig.click(cx, "m:Item 3");
    assert_eq!(rig.picked(), vec!["Item 3"]);
}
