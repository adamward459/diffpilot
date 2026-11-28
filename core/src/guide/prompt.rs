//! The guide prompt is adapted from diffity's `prompts/guide.md` (MIT, see THIRD_PARTY_NOTICES).

use super::build::{
    BULLET_WORDS, CHAPTER_SUMMARY_WORDS, CHECK_WORDS, IMPACT_WORDS, MAX_BULLETS, MAX_IMPACT, NOTE_WORDS,
    OVERVIEW_TITLE_WORDS, PHASE_WORDS, SUMMARY_WORDS, TITLE_WORDS,
};
use super::linked::readable;
use crate::fix::Target;
use crate::run::RunKind;
use crate::runner::RunRequest;
use std::path::PathBuf;

const TEMPLATE: &str = include_str!("prompt.md");

fn linked_section(folders: &[PathBuf]) -> String {
    if folders.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = folders
        .iter()
        .map(|folder| format!("- `{}`", folder.display()))
        .collect();
    format!(
        "\n## Linked folders\n\nThe change depends on code in these folders outside the repo. Read them with Read, Grep and Glob to explain why the change looks the way it does, and to find code there that it may break. They are not part of the diff: add no chapters for their files. A stop in one sets `folder` the same way (see the review path). An `impact` item in one of them sets `folder` to the folder exactly as listed here and `path` to the file inside it.\n\n{}\n",
        lines.join("\n")
    )
}

pub fn render(revisions: &[String], instructions: &str, linked: &[PathBuf]) -> String {
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
        ("check_words", CHECK_WORDS),
        ("impact_words", IMPACT_WORDS),
        ("max_impact", MAX_IMPACT),
    ];
    let mut text = TEMPLATE
        .replace("{{revisions}}", &revisions.join(" "))
        .replace("{{instructions}}", &quoted)
        .replace("{{linked}}", &linked_section(linked));
    for (name, value) in numbers {
        text = text.replace(&format!("{{{{{name}}}}}"), &value.to_string());
    }
    text
}

/// A read-only run that writes the guide of the change under review. `target.model` is the "Reviews and guides"
/// default from the settings. Linked folders that no longer exist are left out.
pub fn request(target: &Target, instructions: &str, linked: &[PathBuf]) -> RunRequest {
    let linked = readable(linked);
    let mut request = RunRequest::new(
        &target.repo,
        target.session.clone(),
        RunKind::Guide,
        "Writing a guide",
        &render(&target.revisions, instructions, &linked),
        target.model,
    );
    request.revisions = target.revisions.clone();
    request.linked = linked;
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_fills_every_placeholder_and_names_every_tool() {
        let prompt = render(&["--merge-base".into(), "main".into()], "", &[]);
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
        assert!(!prompt.contains("Linked folders"));
    }

    #[test]
    fn linked_folders_are_listed_and_passed_to_the_run_only_while_they_exist() {
        let shared = tempfile::tempdir().unwrap();
        let gone = shared.path().join("gone");
        let target = Target {
            repo: shared.path().to_path_buf(),
            session: crate::store::SessionKey::new("/r", "topic", "main"),
            revisions: vec!["HEAD".into()],
            model: crate::review::ReviewModel::STANDARD,
            edit_permission: crate::policy::EditPermission::DontAsk,
        };
        let request = request(&target, "", &[shared.path().to_path_buf(), gone.clone()]);
        assert_eq!(request.linked, [shared.path().to_path_buf()]);
        assert!(request.prompt.contains("## Linked folders"));
        assert!(request.prompt.contains(&format!("- `{}`", shared.path().display())));
        assert!(!request.prompt.contains(&gone.display().to_string()));
    }

    #[test]
    fn the_prompt_states_the_limits_the_tools_enforce() {
        let prompt = render(&["HEAD".into()], "", &[]);
        assert!(prompt.contains(&format!("the overview summary {SUMMARY_WORDS}")));
        assert!(prompt.contains(&format!("a note {NOTE_WORDS}")));
        assert!(prompt.contains(&format!("a check {CHECK_WORDS}")));
    }

    #[test]
    fn a_request_from_the_reviewer_is_quoted() {
        let prompt = render(&["HEAD".into()], "Stress the retry logic.\nSkip tests.", &[]);
        assert!(prompt.contains("> Stress the retry logic.\n> Skip tests.\n"));
    }
}
