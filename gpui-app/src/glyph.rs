use diffpilot_core::file_kind::{FileKind, file_kind};
use gpui::SharedString;
use gpui_component::IconNamed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    ArrowLeftRight,
    Atom,
    Braces,
    Check,
    ChevronDown,
    ChevronRight,
    CircleCheck,
    Eye,
    File,
    FileCode,
    FileText,
    Folder,
    FolderFilled,
    GitBranch,
    List,
    ListFilter,
    ListTree,
    MessageSquare,
    PanelLeft,
    Pencil,
    TextWrap,
}

impl Glyph {
    #[cfg(test)]
    pub const ALL: [Glyph; 21] = [
        Glyph::ArrowLeftRight,
        Glyph::Atom,
        Glyph::Braces,
        Glyph::Check,
        Glyph::ChevronDown,
        Glyph::ChevronRight,
        Glyph::CircleCheck,
        Glyph::Eye,
        Glyph::File,
        Glyph::FileCode,
        Glyph::FileText,
        Glyph::Folder,
        Glyph::FolderFilled,
        Glyph::GitBranch,
        Glyph::List,
        Glyph::ListFilter,
        Glyph::ListTree,
        Glyph::MessageSquare,
        Glyph::PanelLeft,
        Glyph::Pencil,
        Glyph::TextWrap,
    ];

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
        match self {
            Glyph::ArrowLeftRight => "icons/arrow-left-right.svg",
            Glyph::Atom => "icons/atom.svg",
            Glyph::Braces => "icons/braces.svg",
            Glyph::Check => "icons/check.svg",
            Glyph::ChevronDown => "icons/chevron-down.svg",
            Glyph::ChevronRight => "icons/chevron-right.svg",
            Glyph::CircleCheck => "icons/circle-check.svg",
            Glyph::Eye => "icons/eye.svg",
            Glyph::File => "icons/file.svg",
            Glyph::FileCode => "icons/file-code.svg",
            Glyph::FileText => "icons/file-text.svg",
            Glyph::Folder => "icons/folder.svg",
            Glyph::FolderFilled => "icons/folder-filled.svg",
            Glyph::GitBranch => "icons/git-branch.svg",
            Glyph::List => "icons/list.svg",
            Glyph::ListFilter => "icons/list-filter.svg",
            Glyph::ListTree => "icons/list-tree.svg",
            Glyph::MessageSquare => "icons/message-square.svg",
            Glyph::PanelLeft => "icons/panel-left.svg",
            Glyph::Pencil => "icons/pencil.svg",
            Glyph::TextWrap => "icons/text-wrap.svg",
        }
        .into()
    }
}
