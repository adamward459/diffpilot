//! The guide prompt is adapted from diffity's `prompts/guide.md` (MIT, see THIRD_PARTY_NOTICES).

use super::build::{
    BULLET_WORDS, CHAPTER_SUMMARY_WORDS, FLOW_WORDS, IMPACT_WORDS, MAX_BULLETS, MAX_FLOW_STEPS, MAX_IMPACT, NOTE_WORDS,
    OVERVIEW_TITLE_WORDS, PHASE_WORDS, SUMMARY_WORDS, TITLE_WORDS,
};
use crate::fix::Target;
use crate::run::RunKind;
use crate::runner::RunRequest;

const TEMPLATE: &str = include_str!("prompt.md");

pub fn render(revisions: &[String], instructions: &str) -> String {
    let instructions = instructions.trim();
    let quoted = if instructions.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = instructions.lines().map(|line| format!("> {line}")).collect();
        format!(
            "\n## The reviewer's request\n\nThe reviewer asked for this guide with the note below. It decides what to stress.\n\n{}\n",
            lines.join("\n")
        )
    };
    let numbers = [
        ("title_words", TITLE_WORDS),
        ("overview_title_words", OVERVIEW_TITLE_WORDS),
        ("summary_words", SUMMARY_WORDS),
        ("chapter_summary_words", CHAPTER_SUMMARY_WORDS),
        ("note_words", NOTE_WORDS),
        ("bullet_words", BULLET_WORDS),
        ("max_bullets", MAX_BULLETS),
        ("phase_words", PHASE_WORDS),
        ("max_flow_steps", MAX_FLOW_STEPS),
        ("flow_words", FLOW_WORDS),
        ("impact_words", IMPACT_WORDS),
        ("max_impact", MAX_IMPACT),
    ];
    let mut text = TEMPLATE
        .replace("{{revisions}}", &revisions.join(" "))
        .replace("{{instructions}}", &quoted);
    for (name, value) in numbers {
        text = text.replace(&format!("{{{{{name}}}}}"), &value.to_string());
    }
    text
}

/// A read-only run that writes the guide of the change under review. `target.model` is the "Reviews and guides"
/// default from the settings.
pub fn request(target: &Target, instructions: &str) -> RunRequest {
    let mut request = RunRequest::new(
        &target.repo,
        target.session.clone(),
        RunKind::Guide,
        "Writing a guide",
        &render(&target.revisions, instructions),
        target.model,
    );
    request.revisions = target.revisions.clone();
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_fills_every_placeholder_and_names_every_tool() {
        let prompt = render(&["--merge-base".into(), "main".into()], "");
        assert!(!prompt.contains("{{"), "{prompt}");
        assert!(prompt.contains("git diff --merge-base main"));
        for tool in [
            "get_diff",
            "set_overview",
            "add_chapter",
            "add_note",
            "add_diagram",
            "finish_guide",
        ] {
            assert!(prompt.contains(&format!("mcp__diffpilot__{tool}")), "{tool}");
        }
        assert!(!prompt.contains("The reviewer's request"));
    }

    #[test]
    fn the_prompt_states_the_limits_the_tools_enforce() {
        let prompt = render(&["HEAD".into()], "");
        assert!(prompt.contains(&format!("the overview summary {SUMMARY_WORDS}")));
        assert!(prompt.contains(&format!("a note {NOTE_WORDS}")));
        assert!(prompt.contains(&format!("at most {MAX_FLOW_STEPS}")));
    }

    #[test]
    fn a_request_from_the_reviewer_is_quoted() {
        let prompt = render(&["HEAD".into()], "Stress the retry logic.\nSkip tests.");
        assert!(prompt.contains("> Stress the retry logic.\n> Skip tests.\n"));
    }
}
