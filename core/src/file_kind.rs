#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    TypeScript,
    Tsx,
    Swift,
    Markdown,
    Json,
    Yaml,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileTint {
    Blue,
    Teal,
    Orange,
    Yellow,
    Purple,
    Secondary,
}

impl FileKind {
    pub fn tint(self) -> FileTint {
        match self {
            Self::TypeScript => FileTint::Blue,
            Self::Tsx => FileTint::Teal,
            Self::Swift => FileTint::Orange,
            Self::Json => FileTint::Yellow,
            Self::Yaml => FileTint::Purple,
            Self::Markdown | Self::Other => FileTint::Secondary,
        }
    }
}

pub fn file_kind(name: &str) -> FileKind {
    let extension = name
        .rsplit_once('.')
        .map_or(String::new(), |(_, ext)| ext.to_lowercase());
    match extension.as_str() {
        "ts" => FileKind::TypeScript,
        "tsx" => FileKind::Tsx,
        "swift" => FileKind::Swift,
        "md" => FileKind::Markdown,
        "json" => FileKind::Json,
        "yaml" | "yml" => FileKind::Yaml,
        _ => FileKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_extensions_case_insensitively() {
        assert_eq!(file_kind("a.ts"), FileKind::TypeScript);
        assert_eq!(file_kind("A.TSX"), FileKind::Tsx);
        assert_eq!(file_kind("a.swift"), FileKind::Swift);
        assert_eq!(file_kind("README.md"), FileKind::Markdown);
        assert_eq!(file_kind("a.json"), FileKind::Json);
        assert_eq!(file_kind("a.yml"), FileKind::Yaml);
        assert_eq!(file_kind("a.yaml"), FileKind::Yaml);
        assert_eq!(file_kind("Makefile"), FileKind::Other);
    }

    #[test]
    fn tints_follow_the_kind() {
        assert_eq!(file_kind("a.ts").tint(), FileTint::Blue);
        assert_eq!(file_kind("a.swift").tint(), FileTint::Orange);
        assert_eq!(file_kind("a").tint(), FileTint::Secondary);
    }
}
