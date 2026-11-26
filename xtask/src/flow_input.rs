use std::fs;
use std::path::Path;

use crate::{Result, root};

const DIR: &str = "gpui-app/tests/flows";
const BYPASSES: [&str; 3] = [".update(", ".update_in(", ".emit("];

pub fn check() -> Result {
    check_dir(&root().join(DIR))
}

fn check_dir(dir: &Path) -> Result {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        for call in bypasses(&fs::read_to_string(&path)?) {
            found.push(format!("{DIR}/{name} calls `{call}`"));
        }
    }
    if found.is_empty() {
        return Ok(());
    }
    found.sort();
    Err(format!(
        "flows must act like a user: {}. Use the input helpers in diffpilot::testing (click, press, type_text, shows), or add one to gpui-app/src/testing.rs.",
        found.join("; ")
    )
    .into())
}

fn bypasses(source: &str) -> Vec<&'static str> {
    BYPASSES
        .into_iter()
        .filter(|call| {
            source
                .lines()
                .any(|line| !line.trim_start().starts_with("//") && line.contains(call))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flow_that_updates_or_emits_directly_is_flagged() {
        let source = "pill.update(cx, |p, cx| p.go(cx));\nview.update_in(cx, f);\ncx.emit(Event);\n";
        assert_eq!(bypasses(source), [".update(", ".update_in(", ".emit("]);
    }

    #[test]
    fn a_flow_that_only_reads_and_uses_the_helpers_passes() {
        let source = "click(cx, \"a\");\nlet n = entity.read_with(cx, |e, _| e.n());\n// pill.update(cx, f)\n";
        assert!(bypasses(source).is_empty());
    }

    #[test]
    fn the_error_names_the_file_and_the_helpers() {
        let dir = std::env::temp_dir().join(format!("flow-input-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("good.rs"), "click(cx, \"a\");\n").unwrap();
        fs::write(dir.join("bad.rs"), "view.update(cx, f);\n").unwrap();
        let message = check_dir(&dir).unwrap_err().to_string();
        fs::remove_dir_all(&dir).unwrap();
        assert!(message.contains("bad.rs calls `.update(`"), "{message}");
        assert!(!message.contains("good.rs"), "{message}");
        assert!(message.contains("diffpilot::testing"), "{message}");
    }

    #[test]
    fn the_flows_of_this_repo_pass() {
        check().unwrap();
    }
}
