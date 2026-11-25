use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

const ICONS: [(&str, &[u8]); 10] = [
    ("icons/sparkles.svg", include_bytes!("../assets/icons/sparkles.svg")),
    ("icons/check.svg", include_bytes!("../assets/icons/check.svg")),
    (
        "icons/chevron-down.svg",
        include_bytes!("../assets/icons/chevron-down.svg"),
    ),
    ("icons/chevron-up.svg", include_bytes!("../assets/icons/chevron-up.svg")),
    ("icons/copy.svg", include_bytes!("../assets/icons/copy.svg")),
    (
        "icons/loader-circle.svg",
        include_bytes!("../assets/icons/loader-circle.svg"),
    ),
    (
        "icons/circle-check.svg",
        include_bytes!("../assets/icons/circle-check.svg"),
    ),
    ("icons/git-branch.svg", include_bytes!("../assets/icons/git-branch.svg")),
    (
        "icons/triangle-alert.svg",
        include_bytes!("../assets/icons/triangle-alert.svg"),
    ),
    (
        "icons/message-square.svg",
        include_bytes!("../assets/icons/message-square.svg"),
    ),
];

/// The icons the review views draw. An app that already serves these paths does not need it.
pub struct ReviewAssets;

impl AssetSource for ReviewAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

pub const SPARKLES: &str = "icons/sparkles.svg";
pub const CHEVRON_DOWN: &str = "icons/chevron-down.svg";
pub const CHEVRON_UP: &str = "icons/chevron-up.svg";
pub const COPY: &str = "icons/copy.svg";
pub const LOADER: &str = "icons/loader-circle.svg";
pub const CIRCLE_CHECK: &str = "icons/circle-check.svg";
pub const BRANCH: &str = "icons/git-branch.svg";
pub const COMMENT: &str = "icons/message-square.svg";
pub const WARNING: &str = "icons/triangle-alert.svg";
pub const CHECK: &str = "icons/check.svg";
