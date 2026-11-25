use diffpilot_review_ui::ReviewAssets;
use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &str)] = &[
            $((
                concat!("icons/", $name, ".svg"),
                include_str!(concat!("../assets/icons/", $name, ".svg")),
            )),*
        ];
    };
}

icons![
    "arrow-left-right",
    "atom",
    "braces",
    "check",
    "chevron-down",
    "chevron-right",
    "chevron-up",
    "circle-check",
    "eye",
    "file",
    "file-code",
    "file-text",
    "folder",
    "folder-filled",
    "git-branch",
    "list",
    "list-filter",
    "list-tree",
    "message-square",
    "panel-left",
    "pencil",
    "text-wrap",
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let own = ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, svg)| Cow::Borrowed(svg.as_bytes()));
        match own {
            Some(icon) => Ok(Some(icon)),
            None => ReviewAssets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut names: Vec<SharedString> = ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect();
        for name in ReviewAssets.list(path)? {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph::Glyph;
    use gpui_component::{IconName, IconNamed};

    #[test]
    fn the_review_icons_are_served_too() {
        for path in [
            diffpilot_review_ui::assets::SPARKLES,
            diffpilot_review_ui::assets::LOADER,
            diffpilot_review_ui::assets::WARNING,
            diffpilot_review_ui::assets::COPY,
            diffpilot_review_ui::assets::COMMENT,
        ] {
            assert!(Assets.load(path).unwrap().is_some(), "{path}");
            assert!(Assets.list(path).unwrap().iter().any(|name| name.as_ref() == path));
        }
    }

    #[test]
    fn every_glyph_and_menu_icon_has_a_file() {
        let assets = Assets;
        for glyph in Glyph::ALL {
            let path = glyph.path();
            assert!(assets.load(&path).unwrap().is_some(), "{path}");
        }
        for icon in [IconName::Check, IconName::ChevronRight] {
            let path = icon.path();
            assert!(assets.load(&path).unwrap().is_some(), "{path}");
        }
    }
}
