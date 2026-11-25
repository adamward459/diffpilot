use crate::comparison::Comparison;
use crate::diff::FileDiff;
use crate::settings::Settings;
use crate::threads::{Side, ThreadAnchor};
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    MustFix,
    Suggestion,
    Nit,
    Question,
}

impl Severity {
    pub const ALL: [Severity; 4] = [Self::MustFix, Self::Suggestion, Self::Nit, Self::Question];

    pub fn title(self) -> &'static str {
        match self {
            Self::MustFix => "Must fix",
            Self::Suggestion => "Suggestion",
            Self::Nit => "Nit",
            Self::Question => "Question",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MustFix => "must-fix",
            Self::Suggestion => "suggestion",
            Self::Nit => "nit",
            Self::Question => "question",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|severity| severity.as_str() == text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub path: String,
    #[serde(default)]
    pub line: Option<u32>,
    pub severity: Severity,
    pub title: String,
    pub body: String,
}

pub type SeverityCounts = BTreeMap<Severity, usize>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewResult {
    pub summary: String,
    pub findings: Vec<Finding>,
}

impl ReviewResult {
    pub fn counts(&self) -> SeverityCounts {
        let mut counts = SeverityCounts::new();
        for finding in &self.findings {
            *counts.entry(finding.severity).or_insert(0) += 1;
        }
        counts
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelName {
    Opus,
    Sonnet,
    Haiku,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl ModelName {
    pub const ALL: [ModelName; 3] = [Self::Opus, Self::Sonnet, Self::Haiku];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opus => "opus",
            Self::Sonnet => "sonnet",
            Self::Haiku => "haiku",
        }
    }
}

impl Effort {
    pub const ALL: [Effort; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReviewModel {
    pub name: ModelName,
    pub effort: Effort,
}

impl ReviewModel {
    pub const STANDARD: ReviewModel = ReviewModel {
        name: ModelName::Opus,
        effort: Effort::High,
    };

    pub const FIXES: ReviewModel = ReviewModel {
        name: ModelName::Sonnet,
        effort: Effort::Medium,
    };

    pub fn new(name: ModelName, effort: Effort) -> Self {
        Self { name, effort }
    }

    pub fn title(&self) -> String {
        format!(
            "{} · {}",
            capitalized(self.name.as_str()),
            capitalized(self.effort.as_str())
        )
    }
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRequest {
    pub repo: PathBuf,
    pub revisions: Vec<String>,
    pub focus: String,
    pub instructions: String,
    pub paths: Vec<String>,
    pub model: ReviewModel,
}

impl ReviewRequest {
    pub fn new(repo: &Path, revisions: Vec<String>, focus: &str) -> Self {
        Self {
            repo: repo.to_path_buf(),
            revisions,
            focus: focus.to_string(),
            instructions: String::new(),
            paths: Vec::new(),
            model: ReviewModel::STANDARD,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviewScope {
    #[default]
    All,
    OpenFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewRun {
    Idle,
    Running { since: Instant },
    Done { counts: SeverityCounts },
    Failed(String),
}

impl ReviewRun {
    pub fn label(&self) -> String {
        match self {
            Self::Idle => "idle".into(),
            Self::Running { .. } => "running".into(),
            Self::Failed(_) => "failed".into(),
            Self::Done { counts } => {
                let numbers: Vec<String> = Severity::ALL
                    .iter()
                    .map(|severity| counts.get(severity).copied().unwrap_or(0).to_string())
                    .collect();
                format!("done:{}", numbers.join("/"))
            }
        }
    }
}

pub mod focus {
    use super::*;

    pub const PRESETS: [&str; 6] = ["Security", "Performance", "Correctness", "Naming", "Tests", "Types"];
    pub const EVERYTHING: &str = "everything";

    /// Joins the picked presets in preset order. Returns the focus line and the trimmed free text.
    pub fn request(selected: &BTreeSet<String>, text: &str) -> (String, String) {
        let names: Vec<&str> = PRESETS
            .iter()
            .copied()
            .filter(|name| selected.contains(*name))
            .collect();
        let focus = if names.is_empty() {
            EVERYTHING.to_string()
        } else {
            names.join(", ")
        };
        (focus, text.trim().to_string())
    }

    pub fn saved(settings: &Settings, repo: &Path) -> BTreeSet<String> {
        settings
            .review_focuses(repo)
            .into_iter()
            .filter(|name| PRESETS.contains(&name.as_str()))
            .collect()
    }

    pub fn save(settings: &Settings, repo: &Path, selected: &BTreeSet<String>) {
        let ordered = PRESETS
            .iter()
            .filter(|name| selected.contains(**name))
            .map(|name| name.to_string())
            .collect();
        settings.set_review_focuses(repo, ordered);
    }
}

/// What the review popover collects before a run starts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewSetup {
    pub focuses: BTreeSet<String>,
    pub instructions: String,
    pub scope: ReviewScope,
    pub model: Option<ReviewModel>,
}

impl ReviewSetup {
    pub fn load(settings: &Settings, repo: &Path) -> Self {
        Self {
            focuses: focus::saved(settings, repo),
            model: Some(settings.reviews_model()),
            ..Self::default()
        }
    }

    pub fn save(&self, settings: &Settings, repo: &Path) {
        focus::save(settings, repo, &self.focuses);
        if let Some(model) = self.model {
            settings.set_reviews_model(model);
        }
    }

    /// `visible_path` is the file open in the diff; it limits the review when the scope is `OpenFile`.
    pub fn request(&self, repo: &Path, comparison: &Comparison, visible_path: Option<&str>) -> ReviewRequest {
        let (focus, instructions) = focus::request(&self.focuses, &self.instructions);
        let paths = match (self.scope, visible_path) {
            (ReviewScope::OpenFile, Some(path)) => vec![path.to_string()],
            _ => Vec::new(),
        };
        ReviewRequest {
            repo: repo.to_path_buf(),
            revisions: comparison.revisions(),
            focus,
            instructions,
            paths,
            model: self.model.unwrap_or(ReviewModel::STANDARD),
        }
    }
}

pub fn review_title(comparison: &Comparison, branch_name: Option<&str>, file_count: usize) -> String {
    let count = format!("{file_count} {}", if file_count == 1 { "file" } else { "files" });
    match comparison {
        Comparison::Branch { branch, base } => format!("{branch} compared to {base} · {count}"),
        Comparison::CheckedOut { base, .. } => {
            format!(
                "{} compared to {base} · {count}",
                branch_name.unwrap_or("Current branch")
            )
        }
        Comparison::WorkingTree => format!("Working tree compared to HEAD · {count}"),
        Comparison::Staged => format!("Staged changes · {count}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingMeta {
    pub severity: Severity,
    pub title: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedThread {
    pub anchor: ThreadAnchor,
    pub finding: Option<FindingMeta>,
    pub body: String,
}

/// Turns a review result into the threads to show: the summary first, then one per finding.
pub fn plan_threads<F: Borrow<FileDiff>>(result: &ReviewResult, files: &[F]) -> Vec<PlannedThread> {
    let mut planned = Vec::new();
    let summary = result.summary.trim();
    if !summary.is_empty() {
        planned.push(PlannedThread {
            anchor: ThreadAnchor::Change,
            finding: None,
            body: summary.to_string(),
        });
    }
    for finding in &result.findings {
        let (anchor, location) = place(finding, files);
        planned.push(PlannedThread {
            anchor,
            finding: Some(FindingMeta {
                severity: finding.severity,
                title: finding.title.clone(),
                location,
            }),
            body: finding.body.clone(),
        });
    }
    planned
}

fn place<F: Borrow<FileDiff>>(finding: &Finding, files: &[F]) -> (ThreadAnchor, Option<String>) {
    let path = finding.path.strip_prefix("./").unwrap_or(&finding.path);
    let Some(file) = files.iter().map(Borrow::borrow).find(|file| file.path == path) else {
        let place = finding
            .line
            .map_or_else(|| path.to_string(), |line| format!("{path}:{line}"));
        return (ThreadAnchor::Change, Some(place));
    };
    let Some(line) = finding.line else {
        return (ThreadAnchor::file(path), None);
    };
    let shown = file
        .hunks
        .iter()
        .any(|hunk| hunk.lines.iter().any(|shown| shown.new == Some(line)));
    if shown {
        (ThreadAnchor::line(path, Side::New, line), None)
    } else {
        (ThreadAnchor::file(path), Some(format!("Line {line}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes::FileStatus;
    use crate::diff::{DiffLine, Hunk, LineKind};

    fn files() -> Vec<FileDiff> {
        let line = |kind, old, new, text: &str| DiffLine {
            kind,
            old,
            new,
            text: text.into(),
        };
        let mut file = FileDiff::new("src/a.ts", FileStatus::Modified);
        file.additions = 2;
        file.deletions = 1;
        file.hunks.push(Hunk {
            header: "@@ -1,2 +1,3 @@".into(),
            lines: vec![
                line(LineKind::Context, Some(1), Some(1), "keep"),
                line(LineKind::Deleted, Some(2), None, "gone"),
                line(LineKind::Added, None, Some(2), "new"),
            ],
        });
        vec![file]
    }

    fn finding(path: &str, line: Option<u32>, severity: Severity) -> Finding {
        Finding {
            path: path.into(),
            line,
            severity,
            title: "T".into(),
            body: "B".into(),
        }
    }

    fn plan(findings: Vec<Finding>, summary: &str) -> Vec<PlannedThread> {
        plan_threads(
            &ReviewResult {
                summary: summary.into(),
                findings,
            },
            &files(),
        )
    }

    fn anchors(planned: &[PlannedThread]) -> Vec<ThreadAnchor> {
        planned.iter().map(|p| p.anchor.clone()).collect()
    }

    #[test]
    fn a_finding_on_a_shown_line_anchors_to_that_line() {
        let on_new = plan(vec![finding("src/a.ts", Some(2), Severity::Suggestion)], "");
        assert_eq!(anchors(&on_new), [ThreadAnchor::line("src/a.ts", Side::New, 2)]);
        let dotted = plan(vec![finding("./src/a.ts", Some(1), Severity::Suggestion)], "");
        assert_eq!(anchors(&dotted), [ThreadAnchor::line("src/a.ts", Side::New, 1)]);
    }

    #[test]
    fn a_finding_on_a_line_outside_the_diff_anchors_to_the_file_and_keeps_the_line_in_its_label() {
        let planned = plan(vec![finding("src/a.ts", Some(40), Severity::Suggestion)], "");
        assert_eq!(anchors(&planned), [ThreadAnchor::file("src/a.ts")]);
        assert_eq!(
            planned[0].finding.as_ref().unwrap().location.as_deref(),
            Some("Line 40")
        );
    }

    #[test]
    fn a_finding_without_a_line_anchors_to_the_file() {
        let planned = plan(vec![finding("src/a.ts", None, Severity::Suggestion)], "");
        assert_eq!(anchors(&planned), [ThreadAnchor::file("src/a.ts")]);
        assert_eq!(planned[0].finding.as_ref().unwrap().location, None);
    }

    #[test]
    fn a_finding_outside_the_diff_anchors_to_the_change_and_names_its_place() {
        let planned = plan(vec![finding("other.py", Some(3), Severity::Question)], "");
        assert_eq!(anchors(&planned), [ThreadAnchor::Change]);
        assert_eq!(
            planned[0].finding,
            Some(FindingMeta {
                severity: Severity::Question,
                title: "T".into(),
                location: Some("other.py:3".into())
            })
        );
        let no_line = plan(vec![finding("other.py", None, Severity::Question)], "");
        assert_eq!(
            no_line[0].finding.as_ref().unwrap().location.as_deref(),
            Some("other.py")
        );
    }

    #[test]
    fn the_summary_comes_first_as_a_change_thread_without_finding_meta() {
        let planned = plan(vec![finding("src/a.ts", Some(2), Severity::Suggestion)], " All good. ");
        assert_eq!(
            planned[0],
            PlannedThread {
                anchor: ThreadAnchor::Change,
                finding: None,
                body: "All good.".into()
            }
        );
        assert_eq!(planned.len(), 2);
        assert!(plan(vec![], "  ").is_empty());
    }

    #[test]
    fn a_finding_thread_serializes_its_finding_for_a_ui() {
        let meta = FindingMeta {
            severity: Severity::MustFix,
            title: "Bad".into(),
            location: None,
        };
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["severity"], "must-fix");
        assert_eq!(json["title"], "Bad");
    }

    #[test]
    fn counts_findings_per_severity() {
        let result = ReviewResult {
            summary: String::new(),
            findings: vec![
                finding("a", None, Severity::MustFix),
                finding("b", None, Severity::MustFix),
                finding("c", None, Severity::Nit),
            ],
        };
        assert_eq!(
            result.counts(),
            BTreeMap::from([(Severity::MustFix, 2), (Severity::Nit, 1)])
        );
    }

    #[test]
    fn the_run_label_lists_counts_in_severity_order() {
        let counts = BTreeMap::from([(Severity::MustFix, 1), (Severity::Nit, 1)]);
        assert_eq!(ReviewRun::Done { counts }.label(), "done:1/0/1/0");
        assert_eq!(ReviewRun::Idle.label(), "idle");
        assert_eq!(ReviewRun::Running { since: Instant::now() }.label(), "running");
        assert_eq!(ReviewRun::Failed("x".into()).label(), "failed");
    }

    #[test]
    fn focus_request_joins_picked_chips_in_preset_order_and_trims_the_text() {
        let selected = ["Types", "Security", "Bogus"].map(String::from).into();
        assert_eq!(
            focus::request(&selected, "  hi \n"),
            ("Security, Types".to_string(), "hi".to_string())
        );
        assert_eq!(focus::request(&BTreeSet::new(), "").0, "everything");
    }

    #[test]
    fn picked_focuses_are_remembered_per_repo() {
        let settings = Settings::in_memory();
        let a = Path::new("/repo/a");
        let picked = ["Tests", "Security", "Bogus"].map(String::from).into();
        focus::save(&settings, a, &picked);
        assert_eq!(
            focus::saved(&settings, a),
            BTreeSet::from(["Tests".to_string(), "Security".to_string()])
        );
        assert!(focus::saved(&settings, Path::new("/repo/b")).is_empty());
    }

    #[test]
    fn the_last_picked_model_and_effort_are_remembered() {
        let settings = Settings::in_memory();
        assert_eq!(
            ReviewSetup::load(&settings, Path::new("/r")).model,
            Some(ReviewModel::STANDARD)
        );
        let setup = ReviewSetup {
            model: Some(ReviewModel::new(ModelName::Haiku, Effort::Low)),
            ..ReviewSetup::default()
        };
        setup.save(&settings, Path::new("/r"));
        assert_eq!(
            ReviewSetup::load(&settings, Path::new("/r")).model,
            Some(ReviewModel::new(ModelName::Haiku, Effort::Low))
        );
    }

    #[test]
    fn a_setup_builds_the_request_the_run_starts_with() {
        let setup = ReviewSetup {
            focuses: ["Tests".to_string()].into(),
            instructions: " look at auth ".into(),
            scope: ReviewScope::OpenFile,
            model: None,
        };
        let comparison = Comparison::checked_out("main", true);
        let request = setup.request(Path::new("/r"), &comparison, Some("src/a.ts"));
        assert_eq!(request.revisions, ["--merge-base", "main"]);
        assert_eq!(request.focus, "Tests");
        assert_eq!(request.instructions, "look at auth");
        assert_eq!(request.paths, ["src/a.ts"]);
        assert_eq!(request.model, ReviewModel::STANDARD);

        let everything = ReviewSetup::default().request(Path::new("/r"), &comparison, Some("src/a.ts"));
        assert!(everything.paths.is_empty());
        assert_eq!(everything.focus, "everything");
        let no_file = ReviewSetup {
            scope: ReviewScope::OpenFile,
            ..ReviewSetup::default()
        };
        assert!(no_file.request(Path::new("/r"), &comparison, None).paths.is_empty());
    }

    #[test]
    fn titles_the_review_by_comparison() {
        let main = Comparison::checked_out("main", true);
        assert_eq!(
            review_title(&main, Some("feature"), 3),
            "feature compared to main · 3 files"
        );
        assert_eq!(review_title(&main, None, 1), "Current branch compared to main · 1 file");
        assert_eq!(
            review_title(&Comparison::branch("topic", "dev"), None, 0),
            "topic compared to dev · 0 files"
        );
        assert_eq!(
            review_title(&Comparison::WorkingTree, None, 2),
            "Working tree compared to HEAD · 2 files"
        );
        assert_eq!(review_title(&Comparison::Staged, None, 2), "Staged changes · 2 files");
    }

    #[test]
    fn the_model_title_is_capitalized() {
        assert_eq!(ReviewModel::STANDARD.title(), "Opus · High");
    }
}
