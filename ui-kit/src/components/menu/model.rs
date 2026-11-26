use crate::icons::Glyph;
use gpui::{AnyElement, App, Hsla, SharedString, Window};
use std::rc::Rc;

pub type OnSelect = Rc<dyn Fn(&mut Window, &mut App)>;
/// Builds the trailing part of a row. The flag is true while the row is highlighted.
pub type Accessory = Rc<dyn Fn(bool) -> AnyElement>;

pub struct MenuSection {
    pub title: Option<SharedString>,
    pub enabled: bool,
    pub rows: Vec<MenuRow>,
}

impl MenuSection {
    pub fn new(rows: Vec<MenuRow>) -> Self {
        Self {
            title: None,
            enabled: true,
            rows,
        }
    }

    pub fn titled(title: impl Into<SharedString>, rows: Vec<MenuRow>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::new(rows)
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[derive(Clone)]
pub enum MenuRow {
    Item(MenuItem),
    Submenu { title: SharedString, items: Vec<MenuItem> },
}

impl MenuRow {
    pub fn label(&self) -> &SharedString {
        match self {
            MenuRow::Item(item) => &item.label,
            MenuRow::Submenu { title, .. } => title,
        }
    }
}

impl From<MenuItem> for MenuRow {
    fn from(item: MenuItem) -> Self {
        MenuRow::Item(item)
    }
}

#[derive(Clone)]
pub struct MenuItem {
    pub label: SharedString,
    pub detail: Option<SharedString>,
    pub shortcut: Option<SharedString>,
    pub icon: Option<(Glyph, Option<Hsla>)>,
    pub accessory: Option<Accessory>,
    pub checked: bool,
    pub on_select: OnSelect,
}

impl MenuItem {
    pub fn new(label: impl Into<SharedString>, on_select: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self {
            label: label.into(),
            detail: None,
            shortcut: None,
            icon: None,
            accessory: None,
            checked: false,
            on_select: Rc::new(on_select),
        }
    }

    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn icon(mut self, glyph: Glyph, color: Option<Hsla>) -> Self {
        self.icon = Some((glyph, color));
        self
    }

    pub fn accessory(mut self, accessory: impl Fn(bool) -> AnyElement + 'static) -> Self {
        self.accessory = Some(Rc::new(accessory));
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
}

pub const PADDING: f32 = 5.0;
pub const ROW_HEIGHT: f32 = 24.0;
pub const TITLE_HEIGHT: f32 = 22.0;
pub const SEPARATOR_HEIGHT: f32 = 9.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SlotKind {
    Separator,
    Title { section: usize },
    Row { section: usize, row: usize, enabled: bool },
}

/// One line of the menu, with its top edge measured from the top of the first line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub kind: SlotKind,
    pub top: f32,
}

/// Empty sections are left out. Every height is fixed, so a submenu can be placed next to its row
/// without measuring.
pub fn layout(sections: &[MenuSection]) -> Vec<Slot> {
    let mut slots = Vec::new();
    let mut top = 0.0;
    let mut push = |kind: SlotKind, height: f32| {
        slots.push(Slot { kind, top });
        top += height;
    };
    let mut first = true;
    for (section_ix, section) in sections.iter().enumerate() {
        if section.rows.is_empty() {
            continue;
        }
        if !first {
            push(SlotKind::Separator, SEPARATOR_HEIGHT);
        }
        first = false;
        if section.title.is_some() {
            push(SlotKind::Title { section: section_ix }, TITLE_HEIGHT);
        }
        for row_ix in 0..section.rows.len() {
            push(
                SlotKind::Row {
                    section: section_ix,
                    row: row_ix,
                    enabled: section.enabled,
                },
                ROW_HEIGHT,
            );
        }
    }
    slots
}

fn enabled_row(slot: &Slot) -> bool {
    matches!(slot.kind, SlotKind::Row { enabled: true, .. })
}

/// The next enabled row after `from` in `step` direction, wrapping around. With no current row,
/// Down starts at the first row and Up at the last.
pub fn step(slots: &[Slot], from: Option<usize>, forward: bool) -> Option<usize> {
    let count = slots.len();
    if count == 0 {
        return None;
    }
    let start = match (from, forward) {
        (Some(ix), _) => ix,
        (None, true) => count - 1,
        (None, false) => 0,
    };
    (1..=count)
        .map(|offset| {
            if forward {
                (start + offset) % count
            } else {
                (start + count - offset) % count
            }
        })
        .find(|&ix| enabled_row(&slots[ix]))
}

pub fn jump(slots: &[Slot], sections: &[MenuSection], from: Option<usize>, letter: char) -> Option<usize> {
    let count = slots.len();
    let start = from.unwrap_or(count.saturating_sub(1));
    (1..=count).map(|offset| (start + offset) % count).find(|&ix| {
        let SlotKind::Row {
            section,
            row,
            enabled: true,
        } = slots[ix].kind
        else {
            return false;
        };
        sections[section].rows[row]
            .label()
            .chars()
            .next()
            .is_some_and(|first| first.to_lowercase().eq(letter.to_lowercase()))
    })
}
