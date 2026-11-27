use super::model::{MenuSection, OnSelect};
use super::view::Menu;
use crate::components::Presence;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Corner, DismissEvent, ElementId, Entity, EntityId, Focusable, IntoElement, MouseButton,
    MouseDownEvent, Pixels, Point, RenderOnce, SharedString, Window, anchored, canvas, deferred, div, px,
};
use gpui_component::Selectable;
use gpui_component::popover::Popover;
use std::cell::Cell;
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
        arrow: false,
        inline: false,
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
    arrow: bool,
    inline: bool,
}

impl MenuButton {
    pub fn arrow(mut self) -> Self {
        self.arrow = true;
        self
    }

    /// For a menu inside a popover: gpui 0.2.2 panics on a deferred draw inside another one.
    /// The menu paints over earlier siblings only, so open it over the popover's own content.
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

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
        let presence_state =
            window.use_keyed_state(ElementId::Name(format!("{}:presence", self.id).into()), cx, |_, _| {
                Presence::overlay(false)
            });
        let presence = *presence_state.read(cx);
        let view = window.current_view();
        let on_open_change = self.on_open_change.clone();
        let (id, build, min_width) = (self.id.clone(), self.build.clone(), self.min_width);
        let below = matches!(self.anchor, Corner::TopLeft | Corner::TopRight);
        let content_menu = open_menu.clone();
        let arrow_bounds = self.arrow.then(|| {
            window
                .use_keyed_state(ElementId::Name(format!("{}:trigger", self.id).into()), cx, |_, _| {
                    Rc::new(Cell::new(None::<Bounds<Pixels>>))
                })
                .read(cx)
                .clone()
        });
        if self.inline {
            return inline_menu(self, open_menu, presence_state, arrow_bounds, window, cx);
        }
        let popover = Popover::new(ElementId::Name(format!("{}:popover", self.id).into()))
            .appearance(false)
            .overlay_closable(false)
            .open(presence.is_mounted())
            .anchor(self.anchor)
            .trigger(Trigger(self.trigger))
            .on_open_change({
                let open_menu = open_menu.clone();
                move |open, window, cx| {
                    if *open {
                        open_menu.update(cx, |menu, _| *menu = None);
                    }
                    Presence::drive(&presence_state, *open, view, cx);
                    if let Some(callback) = &on_open_change {
                        callback(*open, window, cx);
                    }
                }
            })
            .content({
                let arrow_bounds = arrow_bounds.clone();
                move |_, window, cx| {
                    let sections = build(window, cx);
                    if let Some(menu) = content_menu.read(cx).clone() {
                        menu.update(cx, |menu, cx| {
                            menu.set_sections(sections, cx);
                            menu.set_presence(presence);
                        });
                        return menu;
                    }
                    let popover = cx.entity();
                    new_menu(
                        &content_menu,
                        new_menu_view(&id, sections, min_width, below, &arrow_bounds, presence, cx),
                        move |window, cx| popover.update(cx, |popover, cx| popover.dismiss(window, cx)),
                        window,
                        cx,
                    )
                }
            });
        match arrow_bounds {
            Some(bounds) => div()
                .relative()
                .child(popover)
                .child(
                    canvas(move |area, _, _| bounds.set(Some(area)), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .into_any_element(),
            None => popover.into_any_element(),
        }
    }
}

type MenuSlot = Entity<Option<Entity<Menu>>>;
type ArrowBounds = Option<Rc<Cell<Option<Bounds<Pixels>>>>>;

fn new_menu_view(
    id: &SharedString,
    sections: Vec<MenuSection>,
    min_width: Pixels,
    below: bool,
    arrow: &ArrowBounds,
    presence: Presence,
    cx: &mut App,
) -> Entity<Menu> {
    cx.new(|cx| {
        let mut menu = Menu::new(id.clone(), sections, cx)
            .min_width(min_width)
            .opens_below(below);
        menu.set_presence(presence);
        match arrow {
            Some(trigger) => menu.arrow(trigger.clone()),
            None => menu,
        }
    })
}

fn new_menu(
    slot: &MenuSlot,
    menu: Entity<Menu>,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Menu> {
    menu.focus_handle(cx).focus(window);
    window
        .subscribe(&menu, cx, move |_, _: &DismissEvent, window, cx| {
            on_dismiss(window, cx);
            window.refresh();
        })
        .detach();
    slot.update(cx, |slot, _| *slot = Some(menu.clone()));
    menu
}

fn close_inline(
    presence: &Entity<Presence>,
    view: EntityId,
    on_open_change: &Option<OnOpenChange>,
    window: &mut Window,
    cx: &mut App,
) {
    if presence.read(cx).is_open() {
        Presence::drive(presence, false, view, cx);
        if let Some(callback) = on_open_change {
            callback(false, window, cx);
        }
    }
}

fn inline_menu(
    button: MenuButton,
    slot: MenuSlot,
    presence_state: Entity<Presence>,
    arrow: ArrowBounds,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let MenuButton {
        id,
        trigger,
        anchor,
        min_width,
        build,
        on_open_change,
        ..
    } = button;
    let below = matches!(anchor, Corner::TopLeft | Corner::TopRight);
    let presence = *presence_state.read(cx);
    let view = window.current_view();
    let open = slot.read(cx).clone().filter(|_| presence.is_mounted());
    if let Some(menu) = &open {
        let sections = build(window, cx);
        menu.update(cx, |menu, cx| {
            menu.set_sections(sections, cx);
            menu.set_presence(presence);
        });
    }
    let toggle = {
        let (id, slot, arrow, presence_state) = (id.clone(), slot.clone(), arrow.clone(), presence_state.clone());
        move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
            if presence_state.read(cx).is_open() {
                close_inline(&presence_state, view, &on_open_change, window, cx);
            } else {
                Presence::drive(&presence_state, true, view, cx);
                let opened = *presence_state.read(cx);
                let menu = new_menu_view(&id, build(window, cx), min_width, below, &arrow, opened, cx);
                let (dismiss_state, dismiss_change) = (presence_state.clone(), on_open_change.clone());
                new_menu(
                    &slot,
                    menu,
                    move |window, cx| close_inline(&dismiss_state, view, &dismiss_change, window, cx),
                    window,
                    cx,
                );
                if let Some(callback) = &on_open_change {
                    callback(true, window, cx);
                }
            }
            window.refresh();
        }
    };
    div()
        .relative()
        .child(
            div()
                .id(ElementId::Name(format!("{id}:inline").into()))
                .child(trigger)
                .on_mouse_down(MouseButton::Left, toggle),
        )
        .when_some(open, |this, menu| {
            this.child(
                div()
                    .absolute()
                    .map(|this| match anchor {
                        Corner::TopLeft | Corner::BottomLeft => this.left_0(),
                        Corner::TopRight | Corner::BottomRight => this.right_0(),
                    })
                    .map(|this| if below { this.top_full() } else { this.bottom_full() })
                    .child(
                        anchored().anchor(anchor).snap_to_window_with_margin(px(8.)).child(
                            div()
                                .occlude()
                                .map(|this| if below { this.mt_1() } else { this.mb_1() })
                                .child(menu),
                        ),
                    ),
            )
        })
        .when_some(arrow, |this, bounds| {
            this.child(
                canvas(move |area, _, _| bounds.set(Some(area)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
        })
        .into_any_element()
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
