use super::model::{MenuSection, OnSelect};
use super::view::Menu;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Corner, DismissEvent, ElementId, Entity, Focusable, IntoElement, Pixels, Point, RenderOnce,
    SharedString, Window, anchored, deferred, div, px,
};
use gpui_component::Selectable;
use gpui_component::popover::Popover;
use std::rc::Rc;

type Build = Rc<dyn Fn(&mut Window, &mut App) -> Vec<MenuSection>>;
type OnOpenChange = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// A trigger that opens a glass `Menu` under it. `build` runs on every render while the menu is
/// open, so rows that change while it is open stay current.
pub fn menu_button(
    id: impl Into<SharedString>,
    trigger: impl IntoElement,
    build: impl Fn(&mut Window, &mut App) -> Vec<MenuSection> + 'static,
) -> MenuButton {
    MenuButton {
        id: id.into(),
        trigger: trigger.into_any_element(),
        anchor: Corner::TopLeft,
        min_width: px(180.),
        build: Rc::new(build),
        on_open_change: None,
    }
}

#[derive(IntoElement)]
pub struct MenuButton {
    id: SharedString,
    trigger: AnyElement,
    anchor: Corner,
    min_width: Pixels,
    build: Build,
    on_open_change: Option<OnOpenChange>,
}

impl MenuButton {
    pub fn anchor(mut self, anchor: Corner) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = px(width);
        self
    }

    pub fn on_open_change(mut self, callback: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_change = Some(Rc::new(callback));
        self
    }
}

// Popover wants a `Selectable` trigger. Ours draws its own open look, so the flag is ignored.
#[derive(IntoElement)]
struct Trigger(AnyElement);

impl RenderOnce for Trigger {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.0
    }
}

impl Selectable for Trigger {
    fn selected(self, _: bool) -> Self {
        self
    }

    fn is_selected(&self) -> bool {
        false
    }
}

impl RenderOnce for MenuButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let open_menu = window.use_keyed_state(ElementId::Name(format!("{}:menu", self.id).into()), cx, |_, _| {
            None::<Entity<Menu>>
        });
        let on_open_change = self.on_open_change.clone();
        let (id, build, min_width) = (self.id.clone(), self.build.clone(), self.min_width);
        let below = matches!(self.anchor, Corner::TopLeft | Corner::TopRight);
        let content_menu = open_menu.clone();
        Popover::new(ElementId::Name(format!("{}:popover", self.id).into()))
            .appearance(false)
            .overlay_closable(false)
            .anchor(self.anchor)
            .trigger(Trigger(self.trigger))
            .on_open_change({
                let open_menu = open_menu.clone();
                move |open, window, cx| {
                    if !open {
                        open_menu.update(cx, |menu, _| *menu = None);
                    }
                    if let Some(callback) = &on_open_change {
                        callback(*open, window, cx);
                    }
                }
            })
            .content(move |_, window, cx| {
                let sections = build(window, cx);
                if let Some(menu) = content_menu.read(cx).clone() {
                    menu.update(cx, |menu, cx| menu.set_sections(sections, cx));
                    return menu;
                }
                let menu = cx.new(|cx| {
                    Menu::new(id.clone(), sections, cx)
                        .min_width(min_width)
                        .opens_below(below)
                });
                menu.focus_handle(cx).focus(window);
                let popover = cx.entity();
                window
                    .subscribe(&menu, cx, move |_, _: &DismissEvent, window, cx| {
                        popover.update(cx, |popover, cx| popover.dismiss(window, cx));
                        window.refresh();
                    })
                    .detach();
                content_menu.update(cx, |slot, _| *slot = Some(menu.clone()));
                menu
            })
    }
}

/// A context menu at `position` in window coordinates. The caller shows it while its own state
/// says the menu is open, and `on_dismiss` clears that state.
pub fn menu_at(
    id: impl Into<SharedString>,
    position: Point<Pixels>,
    build: impl Fn(&mut Window, &mut App) -> Vec<MenuSection> + 'static,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> MenuAt {
    MenuAt {
        id: id.into(),
        position,
        build: Rc::new(build),
        on_dismiss: Rc::new(on_dismiss),
    }
}

#[derive(IntoElement)]
pub struct MenuAt {
    id: SharedString,
    position: Point<Pixels>,
    build: Build,
    on_dismiss: OnSelect,
}

impl RenderOnce for MenuAt {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let open_menu = window.use_keyed_state(ElementId::Name(format!("{}:menu", self.id).into()), cx, |_, _| {
            None::<Entity<Menu>>
        });
        let sections = (self.build)(window, cx);
        let menu = match open_menu.read(cx).clone() {
            Some(menu) => {
                menu.update(cx, |menu, cx| menu.set_sections(sections, cx));
                menu
            }
            None => {
                let menu = cx.new(|cx| Menu::new(self.id.clone(), sections, cx));
                menu.focus_handle(cx).focus(window);
                let on_dismiss = self.on_dismiss.clone();
                window
                    .subscribe(&menu, cx, move |_, _: &DismissEvent, window, cx| on_dismiss(window, cx))
                    .detach();
                open_menu.update(cx, |slot, _| *slot = Some(menu.clone()));
                menu
            }
        };
        deferred(
            anchored()
                .position(self.position)
                .snap_to_window_with_margin(px(8.))
                .child(div().occlude().child(menu)),
        )
        .with_priority(2)
    }
}
