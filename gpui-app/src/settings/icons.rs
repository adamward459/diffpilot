use gpui::SharedString;
use std::borrow::Cow;

macro_rules! icons {
    ($($const_name:ident = $name:literal),* $(,)?) => {
        $(pub const $const_name: &str = concat!("settings/", $name, ".svg");)*
        const FILES: &[(&str, &str)] = &[
            $(($const_name, include_str!(concat!("icons/", $name, ".svg")))),*
        ];
    };
}

icons! {
    SUN = "sun",
    CONTRAST = "contrast",
    COLUMNS = "columns-2",
    SPARKLES = "sparkles",
    USERS = "users",
    KEYBOARD = "keyboard",
    LAPTOP = "laptop",
    TERMINAL = "square-terminal",
    UP_DOWN = "chevrons-up-down",
    CHECK = "circle-check",
    ALERT = "circle-alert",
    SPINNER = "loader-circle",
    GITHUB = "github",
}

pub fn load(path: &str) -> Option<Cow<'static, [u8]>> {
    FILES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, svg)| Cow::Borrowed(svg.as_bytes()))
}

pub fn list(prefix: &str) -> Vec<SharedString> {
    FILES
        .iter()
        .filter(|(name, _)| name.starts_with(prefix))
        .map(|(name, _)| SharedString::from(*name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_served() {
        for (name, _) in FILES {
            assert!(load(name).is_some(), "{name}");
        }
        assert_eq!(list("settings/").len(), FILES.len());
        assert!(load("settings/missing.svg").is_none());
    }
}
