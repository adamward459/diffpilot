//! What differs between the kinds of Claude run. Each `RunKind` has one `Activity`, so a new kind is one impl here.

use crate::review::{ReviewModel, ReviewResult};
use crate::run::{Progress, Run, RunKind, RunOutcome};
use crate::settings::Settings;
use crate::store::Store;
use crate::stream::ResultEvent;

pub trait Activity: Sync {
    fn title(&self) -> &'static str;

    /// What the pill says while the run goes, for example "Fixing 1 of 3".
    fn running_label(&self, progress: Option<Progress>) -> String;

    /// Runs that may change files. They get a snapshot first.
    fn edits(&self) -> bool {
        false
    }

    /// The MCP tools the run may call. The bridge enforces the same list.
    fn mcp_tools(&self) -> &'static [&'static str];

    /// The run builds a guide with the MCP tools, so it needs the MCP server and ends when its guide is saved.
    fn writes_guide(&self) -> bool {
        false
    }

    fn model(&self, settings: &Settings) -> ReviewModel;

    /// What the final result line leaves, or why it is not usable.
    fn outcome(&self, result: &ResultEvent) -> Result<Option<RunOutcome>, String> {
        plain_outcome(result)
    }

    /// What a finished run made, for the status line: "4 comments · 7 tool calls".
    fn summary(&self, run: &Run, _store: &Store) -> String {
        format!("{} · {}", plural(run.comments().len(), "comment"), tool_calls(run))
    }
}

impl RunKind {
    pub const ALL: [RunKind; 6] = [
        Self::Review,
        Self::FixComments,
        Self::MakeChanges,
        Self::Reply,
        Self::Guide,
        Self::CommitMessage,
    ];

    pub fn activity(self) -> &'static dyn Activity {
        match self {
            Self::Review => &Review,
            Self::FixComments => &FixComments,
            Self::MakeChanges => &MakeChanges,
            Self::Reply => &Reply,
            Self::Guide => &Guide,
            Self::CommitMessage => &CommitMessage,
        }
    }
}

pub fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

fn tool_calls(run: &Run) -> String {
    plural(run.tool_calls(), "tool call")
}

fn plain_outcome(result: &ResultEvent) -> Result<Option<RunOutcome>, String> {
    if let Some(output) = &result.structured_output {
        return serde_json::from_value::<ReviewResult>(output.clone())
            .map(|review| Some(RunOutcome::Review(review)))
            .map_err(|_| "Claude returned a review in a shape diffpilot cannot read.".into());
    }
    Ok(text_of(result).map(RunOutcome::Text))
}

fn text_of(result: &ResultEvent) -> Option<String> {
    result
        .result
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(String::from)
}

/// A review Claude wrote as JSON text, when the structured output field is missing.
fn review_in_text(text: &str) -> Option<ReviewResult> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(text.get(start..=end)?).ok()
}

const THREAD_TOOLS: &[&str] = &["get_diff", "list_threads", "reply", "resolve", "dismiss"];

struct Review;

impl Activity for Review {
    fn title(&self) -> &'static str {
        "Review"
    }

    fn running_label(&self, _: Option<Progress>) -> String {
        "Reviewing".into()
    }

    fn mcp_tools(&self) -> &'static [&'static str] {
        &["get_diff", "list_threads", "add_comment", "add_general_comment"]
    }

    fn model(&self, settings: &Settings) -> ReviewModel {
        settings.reviews_model()
    }

    fn outcome(&self, result: &ResultEvent) -> Result<Option<RunOutcome>, String> {
        if result.structured_output.is_some() {
            return plain_outcome(result);
        }
        let text = text_of(result).ok_or("Claude returned no review.")?;
        Ok(Some(match review_in_text(&text) {
            Some(review) => RunOutcome::Review(review),
            None => RunOutcome::Text(text),
        }))
    }
}

struct FixComments;

impl Activity for FixComments {
    fn title(&self) -> &'static str {
        "Fix Comments"
    }

    fn running_label(&self, progress: Option<Progress>) -> String {
        match progress {
            Some(Progress { done, total }) => format!("Fixing {done} of {total}"),
            None => "Fixing".into(),
        }
    }

    fn edits(&self) -> bool {
        true
    }

    fn mcp_tools(&self) -> &'static [&'static str] {
        THREAD_TOOLS
    }

    fn model(&self, settings: &Settings) -> ReviewModel {
        settings.fixes_model()
    }
}

struct MakeChanges;

impl Activity for MakeChanges {
    fn title(&self) -> &'static str {
        "Make Changes"
    }

    fn running_label(&self, _: Option<Progress>) -> String {
        "Making changes".into()
    }

    fn edits(&self) -> bool {
        true
    }

    fn mcp_tools(&self) -> &'static [&'static str] {
        &["get_diff", "list_threads"]
    }

    fn model(&self, settings: &Settings) -> ReviewModel {
        settings.fixes_model()
    }
}

struct Reply;

impl Activity for Reply {
    fn title(&self) -> &'static str {
        "Reply"
    }

    fn running_label(&self, _: Option<Progress>) -> String {
        "Replying".into()
    }

    fn edits(&self) -> bool {
        true
    }

    fn mcp_tools(&self) -> &'static [&'static str] {
        THREAD_TOOLS
    }

    fn model(&self, settings: &Settings) -> ReviewModel {
        settings.fixes_model()
    }
}

struct Guide;

impl Activity for Guide {
    fn title(&self) -> &'static str {
        "Guide"
    }

    fn running_label(&self, _: Option<Progress>) -> String {
        "Writing guide".into()
    }

    fn mcp_tools(&self) -> &'static [&'static str] {
        &[
            "get_diff",
            "set_overview",
            "add_chapter",
            "add_note",
            "add_diagram",
            "finish_guide",
        ]
    }

    fn writes_guide(&self) -> bool {
        true
    }

    fn model(&self, settings: &Settings) -> ReviewModel {
        settings.reviews_model()
    }

    fn summary(&self, run: &Run, store: &Store) -> String {
        let chapters = match run.outcome {
            Some(RunOutcome::Guide(guide)) => store
                .load_guide(guide)
                .ok()
                .flatten()
                .map_or(0, |stored| stored.guide.chapters.len()),
            _ => 0,
        };
        format!("{} · {}", plural(chapters, "chapter"), tool_calls(run))
    }
}

struct CommitMessage;

impl Activity for CommitMessage {
    fn title(&self) -> &'static str {
        "Commit message"
    }

    fn running_label(&self, _: Option<Progress>) -> String {
        "Writing commit message".into()
    }

    fn mcp_tools(&self) -> &'static [&'static str] {
        &[]
    }

    fn model(&self, _: &Settings) -> ReviewModel {
        ReviewModel::COMMIT_MESSAGE
    }

    fn summary(&self, run: &Run, _: &Store) -> String {
        tool_calls(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{Effort, ModelName};

    fn result(text: &str, structured: Option<serde_json::Value>) -> ResultEvent {
        ResultEvent {
            is_error: false,
            subtype: "success".into(),
            result: Some(text.into()),
            structured_output: structured,
            num_turns: None,
            duration_ms: None,
            cost_usd: None,
        }
    }

    #[test]
    fn only_review_guide_and_commit_message_runs_are_read_only() {
        let editing: Vec<RunKind> = RunKind::ALL
            .into_iter()
            .filter(|kind| kind.activity().edits())
            .collect();
        assert_eq!(editing, [RunKind::FixComments, RunKind::MakeChanges, RunKind::Reply]);
    }

    #[test]
    fn each_kind_of_run_reads_its_own_model() {
        let settings = Settings::in_memory();
        settings.set_reviews_model(ReviewModel::new(ModelName::Haiku, Effort::Low));
        settings.set_fixes_model(ReviewModel::new(ModelName::Opus, Effort::Medium));
        let model = |kind: RunKind| kind.activity().model(&settings).name;
        assert_eq!(model(RunKind::Review), ModelName::Haiku);
        assert_eq!(model(RunKind::Guide), ModelName::Haiku);
        for kind in [RunKind::FixComments, RunKind::MakeChanges, RunKind::Reply] {
            assert_eq!(model(kind), ModelName::Opus);
        }
        assert_eq!(model(RunKind::CommitMessage), ReviewModel::COMMIT_MESSAGE.name);
    }

    #[test]
    fn a_fix_counts_its_threads_while_it_runs() {
        let label = |progress| RunKind::FixComments.activity().running_label(progress);
        assert_eq!(label(Some(Progress { done: 1, total: 3 })), "Fixing 1 of 3");
        assert_eq!(label(None), "Fixing");
        assert_eq!(RunKind::Guide.activity().running_label(None), "Writing guide");
    }

    #[test]
    fn a_review_reads_findings_written_as_json_text_and_needs_some_answer() {
        let review = RunKind::Review.activity();
        let text = "Here:\n```json\n{\"summary\":\"Fine.\",\"findings\":[]}\n```";
        assert!(matches!(
            review.outcome(&result(text, None)),
            Ok(Some(RunOutcome::Review(_)))
        ));
        assert_eq!(
            review.outcome(&result("Done.", None)),
            Ok(Some(RunOutcome::Text("Done.".into())))
        );
        assert_eq!(
            review.outcome(&result(" ", None)),
            Err("Claude returned no review.".into())
        );
        assert_eq!(
            RunKind::MakeChanges.activity().outcome(&result(" ", None)),
            Ok(None),
            "other runs may end without text"
        );
    }
}
