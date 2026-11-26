use crate::checks::{changed_files, tracked_files};
use crate::features::{Feature, Usage};
use crate::select::select;
use crate::visual::{capture, compare, with_scenes};
use crate::workspace::Workspace;
use crate::{Result, git, root, run};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

const START: &str = "<!-- screenshots -->";
const END: &str = "<!-- /screenshots -->";

pub struct Shots<'a> {
    pub feature: &'a str,
    pub scenes: Vec<String>,
}

pub fn section(repo: &str, sha: &str, base: &str, changed: &[String], shots: &[Shots]) -> String {
    let mut lines = vec![START.to_owned(), "## Screenshots".to_owned(), String::new()];
    lines.push(if changed.is_empty() {
        format!("No image changed vs `{base}`.")
    } else {
        format!("Changed vs `{base}`:")
    });
    lines.extend(
        changed
            .iter()
            .map(|line| format!("- {}", line.split(" (diff: ").next().unwrap_or(line))),
    );
    lines.push(String::new());
    for shot in shots.iter().filter(|s| !s.scenes.is_empty()) {
        lines.push(format!("### {}", shot.feature));
        for scene in &shot.scenes {
            lines.push(format!(
                "![{0}: {1}](https://github.com/{repo}/blob/{sha}/screenshots/{0}/{1}.png?raw=true)",
                shot.feature, scene
            ));
        }
        lines.push(String::new());
    }
    lines.push(END.to_owned());
    lines.join("\n")
}

pub fn with_section(body: &str, block: &str) -> String {
    match (body.find(START), body.find(END)) {
        (Some(start), Some(end)) if start < end => {
            format!("{}{block}{}", &body[..start], &body[end + END.len()..])
        }
        _ => format!("{}\n\n{block}\n", body.trim_end()),
    }
}

fn on_disk(feature: &Feature) -> Result<Shots<'_>> {
    let dir = root().join("screenshots").join(feature.name);
    let mut scenes: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect(),
        Err(_) => vec![],
    };
    scenes.sort();
    Ok(Shots {
        feature: feature.name,
        scenes,
    })
}

fn touched_with_scenes(base: &str) -> Result<Vec<&'static Feature>> {
    let ws = Workspace::load()?;
    let files = changed_files(base)?;
    let usage = Usage::scan(&ws, &tracked_files()?);
    Ok(select(&ws, &usage, &files)
        .into_iter()
        .map(|p| p.feature)
        .filter(|f| !f.scenes.is_empty())
        .collect())
}

pub fn pr_screenshots(rest: &[String]) -> Result {
    let pr: Value = serde_json::from_str(&run(Command::new("gh").args([
        "pr",
        "view",
        "--json",
        "number,baseRefName,body,headRefName",
    ]))?)?;
    let repo = run(Command::new("gh").args(["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"]))?;
    let repo = repo.trim();
    let base_name = pr["baseRefName"].as_str().ok_or("the PR has no base branch")?;
    let head = pr["headRefName"].as_str().ok_or("the PR has no head branch")?;
    let base = format!("origin/{base_name}");
    let features = if rest.is_empty() {
        git(&["fetch", "--quiet", "origin", base_name])?;
        touched_with_scenes(&base)?
    } else {
        with_scenes(rest)?
    };
    if features.is_empty() {
        eprintln!("No touched feature has scenes to capture.");
        return Ok(());
    }
    let names: Vec<&str> = features.iter().map(|f| f.name).collect();

    capture(&features)?;
    let changed = compare(&features, &base)?;

    let mut add = vec!["add".to_owned(), "--".to_owned()];
    add.extend(names.iter().map(|n| format!("screenshots/{n}")));
    git(&add.iter().map(String::as_str).collect::<Vec<_>>())?;
    let staged = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(root())
        .status()?;
    if !staged.success() {
        git(&[
            "commit",
            "-m",
            &format!("docs(screenshots): capture {}", names.join(", ")),
        ])?;
    }
    git(&["push", "origin", &format!("HEAD:{head}")])?;

    let sha = git(&["rev-parse", "HEAD"])?.join("");
    let shots = features.iter().map(|f| on_disk(f)).collect::<Result<Vec<_>>>()?;
    let body = with_section(
        pr["body"].as_str().unwrap_or_default(),
        &section(repo, &sha, &base, &changed, &shots),
    );
    let number = pr["number"].as_u64().ok_or("the PR has no number")?.to_string();
    let mut edit = Command::new("gh")
        .args(["pr", "edit", &number, "--body-file", "-"])
        .current_dir(root())
        .stdin(Stdio::piped())
        .spawn()?;
    edit.stdin.take().ok_or("no stdin")?.write_all(body.as_bytes())?;
    if !edit.wait()?.success() {
        return Err("gh pr edit failed".into());
    }
    eprintln!("Updated PR #{number} with screenshots of {}", names.join(", "));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shots() -> Vec<Shots<'static>> {
        vec![
            Shots {
                feature: "diff-pane",
                scenes: vec!["split-dark".into(), "split-light".into()],
            },
            Shots {
                feature: "review",
                scenes: vec![],
            },
        ]
    }

    #[test]
    fn the_section_groups_images_by_feature_and_lists_changes() {
        let changed = vec![
            "screenshots/diff-pane/split-dark.png: 5 pixels changed (0.1%), max channel delta 9 (diff: /t/x.png)"
                .to_owned(),
        ];
        let text = section("o/r", "abc", "origin/main", &changed, &shots());
        assert_eq!(
            text,
            "<!-- screenshots -->\n## Screenshots\n\nChanged vs `origin/main`:\n\
             - screenshots/diff-pane/split-dark.png: 5 pixels changed (0.1%), max channel delta 9\n\n\
             ### diff-pane\n\
             ![diff-pane: split-dark](https://github.com/o/r/blob/abc/screenshots/diff-pane/split-dark.png?raw=true)\n\
             ![diff-pane: split-light](https://github.com/o/r/blob/abc/screenshots/diff-pane/split-light.png?raw=true)\n\n\
             <!-- /screenshots -->"
        );
    }

    #[test]
    fn an_unchanged_set_says_so() {
        let text = section("o/r", "abc", "origin/main", &[], &shots());
        assert!(text.contains("No image changed vs `origin/main`."));
        assert!(!text.contains("### review"));
    }

    #[test]
    fn the_block_replaces_the_old_one_or_is_appended() {
        let old = "Body\n\n<!-- screenshots -->\nold\n<!-- /screenshots -->\n\nTail";
        assert_eq!(with_section(old, "NEW"), "Body\n\nNEW\n\nTail");
        assert_eq!(with_section("Body\n", "NEW"), "Body\n\nNEW\n");
    }
}
