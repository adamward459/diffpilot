use crate::paths::find_executable;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Editor {
    System,
    VsCode,
    Cursor,
    Zed,
    Sublime,
    Xcode,
    IntelliJ,
    Custom(String),
}

impl Editor {
    pub const PRESETS: [Editor; 7] = [
        Self::VsCode,
        Self::Cursor,
        Self::Zed,
        Self::Sublime,
        Self::Xcode,
        Self::IntelliJ,
        Self::System,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Self::System => "Finder default",
            Self::VsCode => "VS Code",
            Self::Cursor => "Cursor",
            Self::Zed => "Zed",
            Self::Sublime => "Sublime Text",
            Self::Xcode => "Xcode",
            Self::IntelliJ => "IntelliJ IDEA",
            Self::Custom(_) => "Custom…",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTarget {
    pub path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl OpenTarget {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            line: None,
            column: None,
        }
    }

    pub fn at_line(path: impl Into<PathBuf>, line: u32) -> Self {
        Self {
            path: path.into(),
            line: Some(line),
            column: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("Could not start {program}: {source}")]
    Spawn { program: String, source: std::io::Error },
}

/// A custom template without `{file}` gets the file as its last argument.
pub fn command(editor: &Editor, target: &OpenTarget) -> (String, Vec<String>) {
    let file = target.path.to_string_lossy().into_owned();
    let place = |name: &str, args: Vec<String>| (name.to_string(), args);
    let spec = || match (target.line, target.column) {
        (None, _) => file.clone(),
        (Some(line), None) => format!("{file}:{line}"),
        (Some(line), Some(column)) => format!("{file}:{line}:{column}"),
    };
    match editor {
        Editor::System => place("open", vec![file]),
        Editor::VsCode => place("code", vec!["-g".into(), spec()]),
        Editor::Cursor => place("cursor", vec!["-g".into(), spec()]),
        Editor::Zed => place("zed", vec![spec()]),
        Editor::Sublime => place("subl", vec![spec()]),
        Editor::Xcode => match target.line {
            Some(line) => place("xed", vec!["-l".into(), line.to_string(), file]),
            None => place("xed", vec![file]),
        },
        Editor::IntelliJ => match target.line {
            Some(line) => place("idea", vec!["--line".into(), line.to_string(), file]),
            None => place("idea", vec![file]),
        },
        Editor::Custom(template) => custom_command(template, target, &file),
    }
}

fn custom_command(template: &str, target: &OpenTarget, file: &str) -> (String, Vec<String>) {
    let mut words = split_words(template);
    if words.is_empty() {
        return command(&Editor::System, target);
    }
    let has_file = words.iter().any(|word| word.contains("{file}"));
    let line = target.line.unwrap_or(1).to_string();
    let column = target.column.unwrap_or(1).to_string();
    for word in &mut words {
        *word = word
            .replace("{file}", file)
            .replace("{line}", &line)
            .replace("{column}", &column);
    }
    if !has_file {
        words.push(file.to_string());
    }
    let program = words.remove(0);
    (program, words)
}

fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') | (None, '\\') => match chars.next() {
                Some(next) => current.push(next),
                None => current.push('\\'),
            },
            (Some(_), c) => current.push(c),
            (None, '\'' | '"') => {
                quote = Some(ch);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, c) => current.push(c),
        }
    }
    if started || !current.is_empty() {
        words.push(current);
    }
    words
}

const BUNDLE_CLIS: [(&str, &str); 4] = [
    (
        "code",
        "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
    ),
    ("cursor", "/Applications/Cursor.app/Contents/Resources/app/bin/cursor"),
    ("zed", "/Applications/Zed.app/Contents/MacOS/cli"),
    ("subl", "/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl"),
];

fn locate(program: &str) -> Option<PathBuf> {
    let bundled: Vec<PathBuf> = BUNDLE_CLIS
        .iter()
        .filter(|(name, _)| *name == program)
        .map(|(_, path)| PathBuf::from(path))
        .collect();
    if program.contains('/') {
        return Some(PathBuf::from(program));
    }
    find_executable(&[], program).or_else(|| find_executable(&bundled, program))
}

pub fn detect() -> Editor {
    [
        ("code", Editor::VsCode),
        ("cursor", Editor::Cursor),
        ("zed", Editor::Zed),
        ("subl", Editor::Sublime),
    ]
    .into_iter()
    .find(|(program, _)| locate(program).is_some())
    .map_or(Editor::System, |(_, editor)| editor)
}

pub fn open(editor: &Editor, target: &OpenTarget) -> Result<(), OpenError> {
    let (program, args) = command(editor, target);
    let attempt = locate(&program)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not found"))
        .and_then(|path| spawn_detached(&path, &args));
    match attempt {
        Ok(()) => Ok(()),
        Err(_) if program != "open" => {
            let file = target.path.to_string_lossy().into_owned();
            spawn_detached(Path::new("open"), &[file]).map_err(|source| OpenError::Spawn {
                program: program.clone(),
                source,
            })
        }
        Err(source) => Err(OpenError::Spawn { program, source }),
    }
}

fn spawn_detached(program: &Path, args: &[String]) -> std::io::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: Option<u32>, column: Option<u32>) -> OpenTarget {
        OpenTarget {
            path: PathBuf::from("/r/a b.rs"),
            line,
            column,
        }
    }

    fn run(editor: Editor, target: &OpenTarget) -> (String, Vec<String>) {
        command(&editor, target)
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn goto_editors_join_file_line_and_column() {
        for (editor, program, flag) in [
            (Editor::VsCode, "code", true),
            (Editor::Cursor, "cursor", true),
            (Editor::Zed, "zed", false),
            (Editor::Sublime, "subl", false),
        ] {
            let prefix: Vec<&str> = if flag { vec!["-g"] } else { vec![] };
            let expect = |spec: &str| {
                let mut args = prefix.clone();
                args.push(spec);
                (program.to_string(), strings(&args))
            };
            assert_eq!(run(editor.clone(), &at(None, None)), expect("/r/a b.rs"));
            assert_eq!(run(editor.clone(), &at(Some(7), None)), expect("/r/a b.rs:7"));
            assert_eq!(run(editor, &at(Some(7), Some(3))), expect("/r/a b.rs:7:3"));
        }
    }

    #[test]
    fn xcode_passes_the_line_with_a_flag() {
        assert_eq!(
            run(Editor::Xcode, &at(Some(9), None)),
            ("xed".into(), strings(&["-l", "9", "/r/a b.rs"]))
        );
        assert_eq!(
            run(Editor::Xcode, &at(None, None)),
            ("xed".into(), strings(&["/r/a b.rs"]))
        );
    }

    #[test]
    fn intellij_passes_the_line_with_a_flag() {
        assert_eq!(
            run(Editor::IntelliJ, &at(Some(9), Some(2))),
            ("idea".into(), strings(&["--line", "9", "/r/a b.rs"]))
        );
        assert_eq!(
            run(Editor::IntelliJ, &at(None, None)),
            ("idea".into(), strings(&["/r/a b.rs"]))
        );
    }

    #[test]
    fn the_system_editor_ignores_the_line() {
        assert_eq!(
            run(Editor::System, &at(Some(9), Some(2))),
            ("open".into(), strings(&["/r/a b.rs"]))
        );
    }

    #[test]
    fn a_custom_template_fills_its_placeholders() {
        let editor = Editor::Custom("mate -l {line} --col {column} {file}".into());
        assert_eq!(
            run(editor, &at(Some(4), Some(2))),
            ("mate".into(), strings(&["-l", "4", "--col", "2", "/r/a b.rs"]))
        );
    }

    #[test]
    fn a_custom_template_uses_one_for_a_missing_line_and_column() {
        let editor = Editor::Custom("ed +{line}:{column} {file}".into());
        assert_eq!(
            run(editor, &at(None, None)),
            ("ed".into(), strings(&["+1:1", "/r/a b.rs"]))
        );
    }

    #[test]
    fn a_custom_template_keeps_a_path_with_spaces_as_one_argument() {
        let editor = Editor::Custom("\"/Applications/My Editor/bin/ed\" --open={file}".into());
        assert_eq!(
            run(editor, &at(Some(2), None)),
            ("/Applications/My Editor/bin/ed".into(), strings(&["--open=/r/a b.rs"]))
        );
    }

    #[test]
    fn a_custom_template_without_file_appends_the_file() {
        let editor = Editor::Custom("ed --goto {line}".into());
        assert_eq!(
            run(editor, &at(Some(5), None)),
            ("ed".into(), strings(&["--goto", "5", "/r/a b.rs"]))
        );
    }

    #[test]
    fn an_empty_custom_template_falls_back_to_the_system_opener() {
        assert_eq!(
            run(Editor::Custom("   ".into()), &at(Some(5), None)),
            ("open".into(), strings(&["/r/a b.rs"]))
        );
    }

    #[test]
    fn single_quotes_and_escapes_split_like_a_shell() {
        assert_eq!(
            split_words(r#"a 'b c' d\ e "f \" g""#),
            strings(&["a", "b c", "d e", "f \" g"])
        );
        assert_eq!(split_words("a '' b"), strings(&["a", "", "b"]));
    }

    #[test]
    fn editors_round_trip_through_json_and_unknown_values_fail() {
        for editor in [Editor::VsCode, Editor::IntelliJ, Editor::Custom("ed {file}".into())] {
            let json = serde_json::to_string(&editor).unwrap();
            assert_eq!(serde_json::from_str::<Editor>(&json).unwrap(), editor);
        }
        assert_eq!(serde_json::to_string(&Editor::VsCode).unwrap(), "\"vsCode\"");
        assert!(serde_json::from_str::<Editor>("\"emacs\"").is_err());
    }
}
