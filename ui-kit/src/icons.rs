use diffpilot_core::file_kind::{FileKind, file_kind};
use gpui::{AssetSource, Result, SharedString};
use gpui_component::IconNamed;
use std::borrow::Cow;

macro_rules! glyphs {
    ($($variant:ident => $name:literal),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Glyph {
            $($variant),*
        }

        impl Glyph {
            pub const ALL: &[Glyph] = &[$(Glyph::$variant),*];

            pub const fn path(self) -> &'static str {
                match self {
                    $(Glyph::$variant => concat!("icons/", $name, ".svg")),*
                }
            }
        }

        const FILES: &[(&str, &str)] = &[
            $((
                concat!("icons/", $name, ".svg"),
                include_str!(concat!("../assets/icons/", $name, ".svg")),
            )),*
        ];
    };
}

glyphs! {
    ArrowDown => "arrow-down",
    ArrowLeftRight => "arrow-left-right",
    ArrowUp => "arrow-up",
    Atom => "atom",
    BookOpen => "book-open",
    Braces => "braces",
    Check => "check",
    CheckThin => "check-thin",
    ChevronDown => "chevron-down",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    ChevronUp => "chevron-up",
    ChevronsUpDown => "chevrons-up-down",
    CircleAlert => "circle-alert",
    CircleCheck => "circle-check",
    Columns2 => "columns-2",
    Contrast => "contrast",
    Copy => "copy",
    Eye => "eye",
    File => "file",
    FileCode => "file-code",
    FileText => "file-text",
    Folder => "folder",
    FolderFilled => "folder-filled",
    GitBranch => "git-branch",
    GitCommit => "git-commit",
    Github => "github",
    Keyboard => "keyboard",
    Laptop => "laptop",
    List => "list",
    ListFilter => "list-filter",
    ListTree => "list-tree",
    LoaderCircle => "loader-circle",
    MessageSquare => "message-square",
    PanelLeft => "panel-left",
    Pencil => "pencil",
    Rows2 => "rows-2",
    SlidersHorizontal => "sliders-horizontal",
    Sparkles => "sparkles",
    Spinner => "spinner",
    SquareTerminal => "square-terminal",
    Sun => "sun",
    TextWrap => "text-wrap",
    Triangle => "triangle",
    TriangleAlert => "triangle-alert",
    Users => "users",
}

impl Glyph {
    pub fn for_file(name: &str) -> Glyph {
        match file_kind(name) {
            FileKind::TypeScript | FileKind::Swift => Glyph::FileCode,
            FileKind::Tsx => Glyph::Atom,
            FileKind::Markdown => Glyph::FileText,
            FileKind::Json => Glyph::Braces,
            FileKind::Yaml => Glyph::ListTree,
            FileKind::Other => Glyph::File,
        }
    }
}

impl IconNamed for Glyph {
    fn path(self) -> SharedString {
        Glyph::path(self).into()
    }
}

/// Serves every glyph. Pass it to `Application::with_assets`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, svg)| Cow::Borrowed(svg.as_bytes())))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::IconName;

    #[test]
    fn every_glyph_and_menu_icon_has_a_file() {
        for glyph in Glyph::ALL {
            let path = Glyph::path(*glyph);
            assert!(Assets.load(path).unwrap().is_some(), "{path}");
            assert!(Assets.list(path).unwrap().iter().any(|name| name.as_ref() == path));
        }
        for icon in [IconName::Check, IconName::ChevronRight] {
            let path = icon.path();
            assert!(Assets.load(&path).unwrap().is_some(), "{path}");
        }
    }

    #[test]
    fn every_file_has_a_glyph() {
        assert_eq!(FILES.len(), Glyph::ALL.len());
    }
}
