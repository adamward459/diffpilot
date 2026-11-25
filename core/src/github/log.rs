#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Normal,
    Command,
    Error,
    Warning,
    Notice,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub timestamp: Option<String>,
    pub kind: LineKind,
    pub text: String,
}

/// One folded section of the log. It starts at a `##[group]` line and runs to the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogStep {
    pub title: String,
    pub lines: Vec<LogLine>,
}

impl LogStep {
    pub fn has_error(&self) -> bool {
        self.lines.iter().any(|line| line.kind == LineKind::Error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedLog {
    pub steps: Vec<LogStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub step: usize,
    pub line: usize,
}

impl ParsedLog {
    pub fn errors(&self) -> impl Iterator<Item = &LogLine> {
        self.of_kind(LineKind::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &LogLine> {
        self.of_kind(LineKind::Warning)
    }

    fn of_kind(&self, kind: LineKind) -> impl Iterator<Item = &LogLine> {
        self.steps
            .iter()
            .flat_map(|step| &step.lines)
            .filter(move |line| line.kind == kind)
    }

    pub fn first_error(&self) -> Option<&str> {
        self.errors().next().map(|line| line.text.as_str())
    }

    /// Lines that contain `query`, ignoring case, in log order.
    pub fn search(&self, query: &str) -> Vec<Match> {
        if query.is_empty() {
            return Vec::new();
        }
        let needle = query.to_lowercase();
        let mut found = Vec::new();
        for (step_index, step) in self.steps.iter().enumerate() {
            for (line_index, line) in step.lines.iter().enumerate() {
                if line.text.to_lowercase().contains(&needle) {
                    found.push(Match {
                        step: step_index,
                        line: line_index,
                    });
                }
            }
        }
        found
    }
}

pub fn parse_log(text: &str) -> ParsedLog {
    let mut log = ParsedLog::default();
    let mut current: Option<LogStep> = None;
    for raw in text.trim_start_matches('\u{feff}').lines() {
        let (timestamp, rest) = split_timestamp(raw);
        let clean = strip_ansi(rest);
        if let Some(title) = clean.strip_prefix("##[group]") {
            log.steps.extend(current.take());
            current = Some(LogStep {
                title: title.trim().to_string(),
                lines: Vec::new(),
            });
            continue;
        }
        if clean.starts_with("##[endgroup]") {
            continue;
        }
        let (kind, body) = classify(&clean);
        let step = current.get_or_insert_with(|| LogStep {
            title: "Log".to_string(),
            lines: Vec::new(),
        });
        step.lines.push(LogLine {
            timestamp: timestamp.map(String::from),
            kind,
            text: body.to_string(),
        });
    }
    log.steps.extend(current);
    log
}

fn split_timestamp(line: &str) -> (Option<&str>, &str) {
    match line.split_once(' ') {
        Some((head, rest)) if looks_like_timestamp(head) => (Some(head), rest),
        _ => (None, line),
    }
}

fn looks_like_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes.last() == Some(&b'Z')
        && bytes[..4].iter().all(u8::is_ascii_digit)
}

fn classify(line: &str) -> (LineKind, &str) {
    for (prefix, kind) in [
        ("##[error]", LineKind::Error),
        ("##[warning]", LineKind::Warning),
        ("##[notice]", LineKind::Notice),
        ("##[debug]", LineKind::Debug),
        ("[command]", LineKind::Command),
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return (kind, rest);
        }
    }
    (LineKind::Normal, line)
}

pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = include_str!("../../tests/fixtures/github/job_log.txt");

    #[test]
    fn folds_the_log_into_steps() {
        let log = parse_log(LOG);
        let titles: Vec<&str> = log.steps.iter().map(|step| step.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Set up job",
                "Run actions/checkout@v4",
                "Run npm ci",
                "Run npm test",
                "Post job cleanup."
            ]
        );
        assert!(log.steps[3].has_error());
        assert!(!log.steps[2].has_error());
        assert_eq!(
            log.steps[3].lines[0].timestamp.as_deref(),
            Some("2026-11-25T10:01:38.9000000Z")
        );
    }

    #[test]
    fn marks_errors_warnings_and_commands() {
        let log = parse_log(LOG);
        assert_eq!(log.first_error(), Some("Process completed with exit code 1."));
        assert_eq!(log.errors().count(), 2);
        assert_eq!(
            log.warnings().map(|line| line.text.as_str()).collect::<Vec<_>>(),
            ["Node.js 16 is deprecated"]
        );
        let command = log.steps[2]
            .lines
            .iter()
            .find(|line| line.kind == LineKind::Command)
            .unwrap();
        assert_eq!(command.text, "/usr/bin/npm ci");
    }

    #[test]
    fn strips_color_codes_and_the_byte_order_mark() {
        let log = parse_log("\u{feff}2026-11-25T10:00:00.0000000Z \u{1b}[31mred\u{1b}[0m text\n");
        assert_eq!(log.steps[0].title, "Log");
        assert_eq!(log.steps[0].lines[0].text, "red text");
    }

    #[test]
    fn searches_across_steps_in_order() {
        let log = parse_log(LOG);
        let found = log.search("FAIL");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].step, 3);
        assert!(found[0].line < found[1].line);
        assert_eq!(log.search("fail"), found);
        assert!(log.search("").is_empty());
        assert!(log.search("nothing like this").is_empty());
    }

    #[test]
    fn an_empty_log_has_no_steps() {
        assert_eq!(parse_log(""), ParsedLog::default());
    }
}
