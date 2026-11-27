use diffpilot_core::comparison::Comparison;
use diffpilot_core::diff::{FileDiff, Hunk, LineKind};
use diffpilot_core::stage::{self, Action, LineRange, Target};
use diffpilot_core::threads::Side;
use std::collections::HashSet;
use std::path::Path;

type Key<'a> = (bool, &'a str, Option<u32>);

fn key(hunk: &Hunk, index: usize) -> Option<(Key<'_>, LineRange)> {
    let line = &hunk.lines[index];
    match line.kind {
        LineKind::Added => {
            let number = line.new?;
            Some((
                (true, &line.text, Some(number)),
                LineRange::new(Side::New, number, number),
            ))
        }
        LineKind::Deleted => {
            let number = line.old?;
            let anchor = hunk.new_line_near(index);
            Some(((false, &line.text, anchor), LineRange::new(Side::Old, number, number)))
        }
        LineKind::Context => None,
    }
}

/// The shown diff can start from another base than the area a stage action reads, so its line numbers do not
/// carry over. A changed line is found again in `area` by its text and place.
pub fn translate(shown: &FileDiff, selected: &[(u32, u32)], area: &FileDiff) -> Vec<LineRange> {
    let wanted: HashSet<Key> = selected
        .iter()
        .filter_map(|&(hunk, line)| key(&shown.hunks[hunk as usize], line as usize))
        .map(|(key, _)| key)
        .collect();
    area.hunks
        .iter()
        .flat_map(|hunk| (0..hunk.lines.len()).filter_map(|index| key(hunk, index)))
        .filter(|(key, _)| wanted.contains(key))
        .map(|(_, range)| range)
        .collect()
}

/// The numbers as shown, for a file the area diff does not list yet, such as an untracked one.
fn as_shown(shown: &FileDiff, selected: &[(u32, u32)]) -> Vec<LineRange> {
    selected
        .iter()
        .filter_map(|&(hunk, line)| key(&shown.hunks[hunk as usize], line as usize))
        .map(|(_, range)| range)
        .collect()
}

pub fn run(
    repo: &Path,
    comparison: &Comparison,
    action: Action,
    shown: &FileDiff,
    selected: &[(u32, u32)],
) -> Result<(), String> {
    let area = stage::load_area(repo, action.area()).map_err(|error| error.to_string())?;
    let ranges = match area.iter().find(|file| file.path == shown.path) {
        Some(area) => translate(shown, selected, area),
        None if action == Action::Unstage => Vec::new(),
        None => as_shown(shown, selected),
    };
    if ranges.is_empty() {
        return Err(match action {
            Action::Unstage => "None of these lines is staged.",
            _ => "None of these lines has an unstaged change.",
        }
        .into());
    }
    stage::apply(repo, comparison, action, &shown.path, &Target::Lines(ranges)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_pane::testing::{diff, line};

    #[test]
    fn a_changed_line_is_found_again_in_the_area_by_text_and_place() {
        let shown = diff(
            "a.rs",
            vec![
                line(LineKind::Context, Some(1), Some(1), "keep"),
                line(LineKind::Added, None, Some(2), "new"),
                line(LineKind::Deleted, Some(2), None, "gone"),
            ],
        );
        let area = diff(
            "a.rs",
            vec![
                line(LineKind::Added, None, Some(2), "new"),
                line(LineKind::Deleted, Some(7), None, "gone"),
            ],
        );
        assert_eq!(
            translate(&shown, &[(0, 1), (0, 2)], &area),
            [LineRange::new(Side::New, 2, 2), LineRange::new(Side::Old, 7, 7)]
        );
    }

    #[test]
    fn a_line_the_area_lacks_is_dropped() {
        let shown = diff("a.rs", vec![line(LineKind::Added, None, Some(2), "new")]);
        let area = diff("a.rs", vec![line(LineKind::Added, None, Some(3), "new")]);
        assert!(translate(&shown, &[(0, 0)], &area).is_empty());
    }
}
