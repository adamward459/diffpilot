use super::model::{
    MenuItem, MenuRow, MenuSection, SEPARATOR_HEIGHT, Slot, SlotKind, TITLE_HEIGHT, jump, layout, step,
};
use crate::components::interaction::DISABLED_OPACITY;
use crate::components::popover::{arrow_frame, inert};
use crate::components::{
    OVERLAY_DETAIL_TEXT, OVERLAY_PADDING as PADDING, OVERLAY_ROW_HEIGHT as ROW_HEIGHT, OVERLAY_ROW_PADDING,
    OVERLAY_ROW_RADIUS, OVERLAY_TEXT, Presence, glass_panel,
};
use crate::icons::Glyph;
use crate::theme::Theme;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, DismissEvent, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, KeyDownEvent, MouseDownEvent, Pixels, Point, ScrollHandle, SharedString, Subscription, WeakEntity,
    Window, anchored, canvas, div, px, relative,
};
use gpui_component::{Icon, h_flex};
use std::cell::Cell;
use std::rc::Rc;

pub struct Menu {
    id: SharedString,
    sections: Vec<MenuSection>,
    slots: Vec<Slot>,
    highlighted: Option<usize>,
    submenu: Option<OpenSubmenu>,
    parent: Option<WeakEntity<Menu>>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    min_width: Pixels,
    max_height: Pixels,
    opens_below: bool,
    presence: Presence,
    arrow: Option<Rc<Cell<Option<Bounds<Pixels>>>>>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

struct OpenSubmenu {
    slot: usize,
    menu: Entity<Menu>,
    _dismiss: Subscription,
}

impl EventEmitter<DismissEvent> for Menu {}

impl Focusable for Menu {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Menu {
    /// `id` prefixes each row's debug selector as `{id}:{label}`.
    pub fn new(id: impl Into<SharedString>, sections: Vec<MenuSection>, cx: &mut Context<Self>) -> Self {
        Self {
            id: id.into(),
            slots: layout(&sections),
            sections,
            highlighted: None,
            submenu: None,
            parent: None,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            min_width: px(180.),
            max_height: px(520.),
            opens_below: true,
            presence: Presence::entering(),
            arrow: None,
            bounds: Rc::default(),
        }
    }

    pub fn min_width(mut self, width: Pixels) -> Self {
        self.min_width = width;
        self
    }

    /// The open state of the popup that holds this menu. A menu without one opens when it is drawn.
    pub fn set_presence(&mut self, presence: Presence) {
        self.presence = presence;
    }

    pub fn opens_below(mut self, below: bool) -> Self {
        self.opens_below = below;
        self
    }

    pub fn arrow(mut self, trigger: Rc<Cell<Option<Bounds<Pixels>>>>) -> Self {
        self.arrow = Some(trigger);
        self
    }

    pub fn max_height(mut self, height: Pixels) -> Self {
        self.max_height = height;
        self
    }

    pub fn highlighted_label(&self) -> Option<SharedString> {
        self.highlighted
            .and_then(|ix| self.row_at(ix))
            .map(|row| row.label().clone())
    }

    pub fn open_submenu(&self) -> Option<Entity<Menu>> {
        self.submenu.as_ref().map(|open| open.menu.clone())
    }

    /// Swaps in fresh rows while the menu stays open. The highlight and an open submenu stay
    /// when their row is still there.
    pub fn set_sections(&mut self, sections: Vec<MenuSection>, cx: &mut Context<Self>) {
        let highlighted = self.highlighted_label();
        let submenu_title = self
            .submenu
            .as_ref()
            .and_then(|open| self.row_at(open.slot))
            .map(|row| row.label().clone());
        self.slots = layout(&sections);
        self.sections = sections;
        let find = |this: &Self, label: &SharedString| {
            this.slots.iter().position(|slot| {
                matches!(slot.kind, SlotKind::Row { .. }) && this.row_at_slot(slot).map(MenuRow::label) == Some(label)
            })
        };
        self.highlighted = highlighted.and_then(|label| find(self, &label));
        match submenu_title.and_then(|title| find(self, &title)) {
            Some(slot) => {
                let items = match self.row_at(slot) {
                    Some(MenuRow::Submenu { items, .. }) => items.clone(),
                    _ => Vec::new(),
                };
                if let Some(open) = self.submenu.as_mut() {
                    open.slot = slot;
                    open.menu.update(cx, |menu, cx| {
                        menu.set_sections(vec![MenuSection::new(rows_of(items))], cx)
                    });
                }
            }
            None => self.submenu = None,
        }
    }

    fn contains(&self, position: &Point<Pixels>, cx: &App) -> bool {
        self.bounds.get().contains(position)
            || self
                .submenu
                .as_ref()
                .is_some_and(|open| open.menu.read(cx).contains(position, cx))
    }

    fn mouse_down_out(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.parent.is_none() && !self.contains(&event.position, cx) {
            cx.emit(DismissEvent);
        }
    }

    fn row_at_slot(&self, slot: &Slot) -> Option<&MenuRow> {
        match slot.kind {
            SlotKind::Row { section, row, .. } => self.sections.get(section)?.rows.get(row),
            _ => None,
        }
    }

    fn row_at(&self, ix: usize) -> Option<&MenuRow> {
        self.row_at_slot(self.slots.get(ix)?)
    }

    fn is_enabled(&self, ix: usize) -> bool {
        matches!(
            self.slots.get(ix).map(|slot| slot.kind),
            Some(SlotKind::Row { enabled: true, .. })
        )
    }

    fn highlight(&mut self, ix: Option<usize>, cx: &mut Context<Self>) {
        self.highlighted = ix;
        if let Some(ix) = ix {
            self.scroll_to(ix);
        }
        cx.notify();
    }

    fn scroll_to(&self, ix: usize) {
        let Some(slot) = self.slots.get(ix) else { return };
        let view = self.scroll.bounds().size.height;
        if view <= px(0.) {
            return;
        }
        let offset = self.scroll.offset();
        let visible_top = -offset.y;
        let top = px(slot.top);
        let bottom = px(slot.top + ROW_HEIGHT + 2.0 * PADDING);
        if top < visible_top {
            self.scroll.set_offset(gpui::point(offset.x, -top));
        } else if bottom > visible_top + view {
            self.scroll.set_offset(gpui::point(offset.x, -(bottom - view)));
        }
    }

    fn hover(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_enabled(ix) || self.highlighted == Some(ix) {
            return;
        }
        self.highlighted = Some(ix);
        match self.row_at(ix) {
            Some(MenuRow::Submenu { .. }) => self.show_submenu(ix, false, window, cx),
            _ => self.submenu = None,
        }
        cx.notify();
    }

    fn show_submenu(&mut self, ix: usize, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.submenu.as_ref().is_some_and(|open| open.slot == ix) {
            if focus {
                self.focus_submenu(window, cx);
            }
            return;
        }
        let Some(MenuRow::Submenu { title, items }) = self.row_at(ix).cloned() else {
            return;
        };
        let parent = cx.entity().downgrade();
        let id = SharedString::from(format!("{}:{title}", self.id));
        let (min_width, max_height) = (self.min_width, self.max_height);
        let menu = cx.new(|cx| {
            let mut menu = Menu::new(id, vec![MenuSection::new(rows_of(items))], cx)
                .min_width(min_width)
                .max_height(max_height);
            menu.parent = Some(parent);
            menu
        });
        let dismiss = cx.subscribe(&menu, |_, _, _: &DismissEvent, cx| cx.emit(DismissEvent));
        self.submenu = Some(OpenSubmenu {
            slot: ix,
            menu,
            _dismiss: dismiss,
        });
        if focus {
            self.focus_submenu(window, cx);
        }
        cx.notify();
    }

    fn focus_submenu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = &self.submenu else { return };
        open.menu.update(cx, |menu, cx| {
            if menu.highlighted.is_none() {
                let first = step(&menu.slots, None, true);
                menu.highlight(first, cx);
            }
            menu.focus.focus(window);
        });
    }

    fn close_submenu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submenu.take().is_some() {
            self.focus.focus(window);
            cx.notify();
        }
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_enabled(ix) {
            return;
        }
        match self.row_at(ix).cloned() {
            Some(MenuRow::Item(item)) => {
                (item.on_select)(window, cx);
                cx.emit(DismissEvent);
            }
            Some(MenuRow::Submenu { .. }) => {
                self.highlighted = Some(ix);
                self.show_submenu(ix, true, window, cx);
            }
            None => {}
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt {
            return;
        }
        match keystroke.key.as_str() {
            "down" => self.highlight(step(&self.slots, self.highlighted, true), cx),
            "up" => self.highlight(step(&self.slots, self.highlighted, false), cx),
            "enter" | "space" => {
                if let Some(ix) = self.highlighted {
                    self.confirm(ix, window, cx);
                }
            }
            "right" => {
                if let Some(ix) = self
                    .highlighted
                    .filter(|&ix| matches!(self.row_at(ix), Some(MenuRow::Submenu { .. })))
                {
                    self.show_submenu(ix, true, window, cx);
                }
            }
            "left" => match self.parent.as_ref().and_then(WeakEntity::upgrade) {
                Some(parent) => parent.update(cx, |parent, cx| parent.close_submenu(window, cx)),
                None => return,
            },
            "escape" => cx.emit(DismissEvent),
            key => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(letter), None) if letter.is_alphanumeric() => {
                        if let Some(ix) = jump(&self.slots, &self.sections, self.highlighted, letter) {
                            self.highlight(Some(ix), cx);
                        }
                    }
                    _ => return,
                }
            }
        }
        cx.stop_propagation();
    }

    fn render_slot(&self, ix: usize, slot: &Slot, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        match slot.kind {
            SlotKind::Separator => div()
                .h(px(SEPARATOR_HEIGHT))
                .px(px(8.))
                .flex()
                .items_center()
                .child(div().h(px(1.)).w_full().bg(theme.ui.line))
                .into_any_element(),
            SlotKind::Title { section } => div()
                .h(px(TITLE_HEIGHT))
                .px(px(10.))
                .pt(px(6.))
                .text_size(crate::theme::text_px(crate::theme::text_size::SMALL))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.ui.muted)
                .whitespace_nowrap()
                .child(self.sections[section].title.clone().unwrap_or_default())
                .into_any_element(),
            SlotKind::Row { enabled, .. } => {
                let row = self.row_at(ix).expect("a row slot points at a row");
                self.render_row(ix, row, enabled, theme, cx)
            }
        }
    }

    fn render_row(&self, ix: usize, row: &MenuRow, enabled: bool, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let on = enabled && self.highlighted == Some(ix);
        let text = if on { theme.on_accent } else { theme.ui.text };
        let muted = if on { theme.on_accent_dim } else { theme.ui.muted };
        let (item, is_submenu) = match row {
            MenuRow::Item(item) => (Some(item), false),
            MenuRow::Submenu { .. } => (None, true),
        };
        let checked = item.is_some_and(|item| item.checked);
        let selector = format!("{}:{}", self.id, row.label());
        h_flex()
            .id(ElementId::NamedInteger(self.id.clone(), ix as u64))
            .debug_selector(move || selector)
            .h(px(ROW_HEIGHT))
            .px(px(OVERLAY_ROW_PADDING))
            .gap(px(6.))
            .rounded(px(OVERLAY_ROW_RADIUS))
            .text_size(crate::theme::text_px(OVERLAY_TEXT))
            .text_color(text)
            .whitespace_nowrap()
            .when(on, |this| this.bg(theme.accent))
            .when(!enabled, |this| this.opacity(DISABLED_OPACITY))
            .when(enabled, |this| {
                this.cursor_pointer()
                    .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                        if *hovered {
                            this.hover(ix, window, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| this.confirm(ix, window, cx)))
            })
            .child(div().w(px(14.)).flex_none().when(checked, |this| {
                this.child(Icon::new(Glyph::Check).size(px(12.)).text_color(text))
            }))
            .when_some(item.and_then(|item| item.icon), |this, (glyph, color)| {
                this.child(
                    Icon::new(glyph)
                        .size(px(15.))
                        .flex_none()
                        .text_color(color.unwrap_or(text)),
                )
            })
            .child(div().flex_shrink().min_w_0().truncate().child(row.label().clone()))
            .when_some(item.and_then(|item| item.detail.clone()), |this, detail| {
                this.child(div().flex_shrink().min_w_0().truncate().text_color(muted).child(detail))
            })
            .child(div().flex_1().min_w(px(12.)))
            .when_some(item.and_then(|item| item.accessory.clone()), |this, accessory| {
                this.child(accessory(on))
            })
            .when_some(item.and_then(|item| item.shortcut.clone()), |this, shortcut| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(crate::theme::text_px(OVERLAY_DETAIL_TEXT))
                        .text_color(muted)
                        .child(shortcut),
                )
            })
            .when(is_submenu, |this| {
                this.child(
                    Icon::new(Glyph::ChevronRight)
                        .size(px(12.))
                        .flex_none()
                        .text_color(muted),
                )
            })
            .into_any_element()
    }
}

fn rows_of(items: Vec<MenuItem>) -> Vec<MenuRow> {
    items.into_iter().map(MenuRow::Item).collect()
}

impl Render for Menu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let slots = self.slots.clone();
        let rows: Vec<AnyElement> = slots
            .iter()
            .enumerate()
            .map(|(ix, slot)| self.render_slot(ix, slot, &theme, cx))
            .collect();
        // The submenu sits beside the scroll area, not inside it, because a scroll area clips its children.
        let submenu = self
            .submenu
            .as_ref()
            .filter(|_| !self.presence.is_closing())
            .map(|open| {
                let top = slots[open.slot].top + f32::from(self.scroll.offset().y);
                div().absolute().left(relative(1.)).top(px(top)).child(
                    anchored()
                        .snap_to_window_with_margin(px(8.))
                        .child(div().occlude().child(open.menu.clone())),
                )
            });
        let bounds = self.bounds.clone();
        let panel = glass_panel(&theme).child(
            crate::components::scroll_y(
                &self.scroll,
                div().id(ElementId::Name(self.id.clone())).p(px(PADDING)).children(rows),
            )
            .min_w(self.min_width)
            .max_w(px(520.))
            .max_h(self.max_height),
        );
        div()
            .relative()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down_out(cx.listener(Self::mouse_down_out))
            .child(self.presence.slide(
                format!("{}:open", self.id),
                match &self.arrow {
                    Some(trigger) => arrow_frame(panel, self.opens_below, trigger.clone()),
                    None => div().child(panel),
                },
                self.opens_below,
            ))
            .children(self.presence.is_closing().then(inert))
            .child(
                canvas(move |area, _, _| bounds.set(area), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .children(submenu)
    }
}
