use crate::review::ReviewRequest;

const TEMPLATE: &str = include_str!("review_prompt.md");

pub fn render(request: &ReviewRequest) -> String {
    TEMPLATE
        .replace("{{revisions}}", &diff_arguments(request))
        .replace("{{focus}}", &request.focus)
        .replace(
            "{{instructions}}",
            &(instructions(&request.instructions) + &scope(&request.paths)),
        )
}

pub fn diff_arguments(request: &ReviewRequest) -> String {
    let revisions = request.revisions.join(" ");
    if request.paths.is_empty() {
        return revisions;
    }
    let quoted: Vec<String> = request.paths.iter().map(|path| format!("'{path}'")).collect();
    format!("{revisions} -- {}", quoted.join(" "))
}

pub fn scope(paths: &[String]) -> String {
    if paths.is_empty() {
        return String::new();
    }
    let list: Vec<String> = paths.iter().map(|path| format!("- `{path}`")).collect();
    format!(
        "\n## Scope\n\nReview only these files. You may read other files for context, but do not report findings on them.\n\n{}\n",
        list.join("\n")
    )
}

pub fn instructions(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let quoted: Vec<String> = trimmed.split('\n').map(|line| format!("> {line}")).collect();
    format!(
        "\n## The user's instructions\n\nThe user asked for this review with the guidance below. It takes priority over the generic passes in Step 2: spend most of the review on it, and say in your final summary how you addressed it.\n\n{}\n",
        quoted.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn request(revisions: &[&str], focus: &str) -> ReviewRequest {
        ReviewRequest::new(
            Path::new("/r"),
            revisions.iter().map(|r| r.to_string()).collect(),
            focus,
        )
    }

    #[test]
    fn a_scoped_review_diffs_only_those_files_and_says_so() {
        let mut scoped = request(&["--merge-base", "main"], "Tests");
        scoped.paths = vec!["src/a b.ts".into()];
        let prompt = render(&scoped);
        assert!(prompt.contains("git diff --merge-base main -- 'src/a b.ts'"));
        assert!(prompt.contains("## Scope"));
        assert!(!prompt.contains("{{"));
    }

    #[test]
    fn the_rendered_prompt_has_no_placeholders_left() {
        let plain = render(&request(&["--cached"], "everything"));
        assert!(!plain.contains("{{"));
        assert!(plain.contains("git diff --cached"));
        assert!(plain.contains("Focus: everything"));
        assert!(!plain.contains("The user's instructions"));

        let mut guided = request(&["HEAD"], "Tests, Types");
        guided.instructions = "Look at auth.\nAnd retries.".into();
        let guided = render(&guided);
        assert!(!guided.contains("{{"));
        assert!(guided.contains("> Look at auth.\n> And retries.\n"));
        assert!(guided.contains("takes priority over the generic passes in Step 2"));
    }
}
