use std::fs;
use std::process::Command;

use crate::{Result, root, run};

const FILE: &str = "CHANGELOG.md";
const MAX_LINE: usize = 250;

pub fn check() -> Result {
    let text = fs::read_to_string(root().join(FILE))?;
    let long: Vec<String> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| line.chars().count() > MAX_LINE)
        .map(|(i, _)| format!("{FILE}:{}", i + 1))
        .collect();
    if long.is_empty() {
        return Ok(());
    }
    Err(format!("lines over {MAX_LINE} characters: {}", long.join(", ")).into())
}

pub fn release_notes(rest: &[String]) -> Result {
    let [version] = rest else {
        return Err("usage: cargo xtask release-notes <version>".into());
    };
    check()?;
    let text = fs::read_to_string(root().join(FILE))?;
    let heading = format!("## [{}]", version.trim_start_matches('v'));
    let notes: Vec<&str> = text
        .lines()
        .skip_while(|line| !line.starts_with(&heading))
        .skip(1)
        .take_while(|line| !line.starts_with("## [") && !line.starts_with('['))
        .collect();
    let notes = notes.join("\n");
    if notes.trim().is_empty() {
        return Err(format!("{FILE} has no notes under {heading}").into());
    }
    println!("{}", notes.trim());
    Ok(())
}

const REPO_URL: &str = "https://github.com/adamward459/diffpilot";
const MANIFEST: &str = "gpui-app/Cargo.toml";

pub fn release(rest: &[String]) -> Result {
    let [version] = rest else {
        return Err("usage: cargo xtask release X.Y.Z".into());
    };
    let is_semver = version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if !is_semver {
        return Err(format!("{version} is not a version like 1.2.3").into());
    }
    let date = run(Command::new("date").args(["-u", "+%Y-%m-%d"]))?.trim().to_owned();
    let changelog = root().join(FILE);
    fs::write(
        &changelog,
        cut_release(&fs::read_to_string(&changelog)?, version, &date)?,
    )?;
    let manifest = root().join(MANIFEST);
    fs::write(&manifest, set_version(&fs::read_to_string(&manifest)?, version)?)?;
    // Resolving the workspace rewrites the diffpilot entry in Cargo.lock.
    run(Command::new("cargo").args(["metadata", "--format-version", "1"])).map(drop)
}

fn is_link(line: &str) -> bool {
    line.starts_with('[') && line.contains("]: ")
}

fn cut_release(text: &str, version: &str, date: &str) -> Result<String> {
    let lines: Vec<&str> = text.lines().collect();
    let head = lines
        .iter()
        .position(|line| *line == "## [Unreleased]")
        .ok_or(format!("{FILE} has no ## [Unreleased] section"))?;
    if lines.iter().any(|line| line.starts_with(&format!("## [{version}]"))) {
        return Err(format!("{FILE} already has a {version} section").into());
    }
    let end = lines[head + 1..]
        .iter()
        .position(|line| line.starts_with("## [") || is_link(line))
        .map_or(lines.len(), |i| head + 1 + i);
    if lines[head + 1..end].iter().all(|line| line.trim().is_empty()) {
        return Err(format!("{FILE} has nothing under ## [Unreleased]").into());
    }
    let previous = lines[end..]
        .iter()
        .find_map(|line| line.strip_prefix("## [")?.split_once(']').map(|(v, _)| v));

    let mut out: Vec<String> = vec![];
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("[Unreleased]: ") {
            continue;
        }
        out.push(line.to_string());
        if i == head {
            out.push(String::new());
            out.push(format!("## [{version}] - {date}"));
        }
    }
    let links = [
        format!("[Unreleased]: {REPO_URL}/compare/v{version}...HEAD"),
        match previous {
            Some(prev) => format!("[{version}]: {REPO_URL}/compare/v{prev}...v{version}"),
            None => format!("[{version}]: {REPO_URL}/releases/tag/v{version}"),
        },
    ];
    match out.iter().position(|line| is_link(line)) {
        Some(i) => {
            out.splice(i..i, links);
        }
        None => {
            while out.last().is_some_and(|line| line.trim().is_empty()) {
                out.pop();
            }
            out.push(String::new());
            out.extend(links);
        }
    }
    Ok(out.join("\n") + "\n")
}

fn set_version(manifest: &str, version: &str) -> Result<String> {
    let mut done = false;
    let lines: Vec<String> = manifest
        .lines()
        .map(|line| {
            if !done && line.starts_with("version = \"") {
                done = true;
                format!("version = \"{version}\"")
            } else {
                line.to_owned()
            }
        })
        .collect();
    if !done {
        return Err(format!("{MANIFEST} has no version line").into());
    }
    Ok(lines.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: &str = "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- A thing.\n\n[Unreleased]: https://github.com/adamward459/diffpilot/commits/main\n";

    #[test]
    fn the_first_release_takes_the_unreleased_notes_and_links_to_its_tag() {
        let out = cut_release(FIRST, "0.1.0", "2026-11-26").unwrap();
        assert_eq!(
            out,
            "# Changelog\n\n## [Unreleased]\n\n## [0.1.0] - 2026-11-26\n\n### Added\n\n- A thing.\n\n\
             [Unreleased]: https://github.com/adamward459/diffpilot/compare/v0.1.0...HEAD\n\
             [0.1.0]: https://github.com/adamward459/diffpilot/releases/tag/v0.1.0\n"
        );
    }

    #[test]
    fn a_later_release_compares_with_the_previous_version() {
        let first = cut_release(FIRST, "0.1.0", "2026-11-26").unwrap();
        let next = first.replace("## [Unreleased]\n", "## [Unreleased]\n\n### Fixed\n\n- A bug.\n");
        let out = cut_release(&next, "0.2.0", "2026-12-01").unwrap();
        assert!(out.contains(
            "## [Unreleased]\n\n## [0.2.0] - 2026-12-01\n\n### Fixed\n\n- A bug.\n\n## [0.1.0] - 2026-11-26"
        ));
        assert!(out.ends_with(
            "[Unreleased]: https://github.com/adamward459/diffpilot/compare/v0.2.0...HEAD\n\
             [0.2.0]: https://github.com/adamward459/diffpilot/compare/v0.1.0...v0.2.0\n\
             [0.1.0]: https://github.com/adamward459/diffpilot/releases/tag/v0.1.0\n"
        ));
    }

    #[test]
    fn an_empty_unreleased_section_or_a_repeated_version_is_refused() {
        let released = cut_release(FIRST, "0.1.0", "2026-11-26").unwrap();
        assert!(cut_release(&released, "0.2.0", "2026-12-01").is_err());
        let again = released.replace("## [Unreleased]\n", "## [Unreleased]\n\n- More.\n");
        assert!(cut_release(&again, "0.1.0", "2026-12-01").is_err());
    }

    #[test]
    fn only_the_package_version_changes() {
        let manifest =
            "[package]\nname = \"diffpilot\"\nversion = \"0.1.0\"\n\n[dependencies]\ncore = { version = \"0.1.0\" }\n";
        assert_eq!(
            set_version(manifest, "0.2.0").unwrap(),
            "[package]\nname = \"diffpilot\"\nversion = \"0.2.0\"\n\n[dependencies]\ncore = { version = \"0.1.0\" }\n"
        );
    }
}
