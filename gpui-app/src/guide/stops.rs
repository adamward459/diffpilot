use diffpilot_core::guide::Note;

/// The reviewable notes of one chapter, as indices into the stored `Chapter::notes`. The indices stay stable when
/// a note drops out because its file left the diff.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stops(Vec<usize>);

impl Stops {
    /// A linked note counts. Any other note counts while its file is one of the chapter's files.
    pub fn of(notes: &[Note], files: &[String]) -> Self {
        Self(
            notes
                .iter()
                .enumerate()
                .filter(|(_, note)| note.is_linked() || files.contains(&note.path))
                .map(|(index, _)| index)
                .collect(),
        )
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().copied()
    }

    pub fn contains(&self, note: usize) -> bool {
        self.0.contains(&note)
    }

    /// The 1-based position of `note` on the path.
    pub fn number(&self, note: usize) -> Option<usize> {
        self.0.iter().position(|&index| index == note).map(|at| at + 1)
    }

    pub fn next(&self, note: usize) -> Option<usize> {
        let at = self.0.iter().position(|&index| index == note)?;
        self.0.get(at + 1).copied()
    }

    pub fn previous(&self, note: usize) -> Option<usize> {
        let at = self.0.iter().position(|&index| index == note)?;
        at.checked_sub(1).map(|before| self.0[before])
    }

    /// The first stop that is not done, or the first stop when all are done.
    pub fn entry(&self, done: impl Fn(usize) -> bool) -> Option<usize> {
        self.0
            .iter()
            .copied()
            .find(|&note| !done(note))
            .or_else(|| self.0.first().copied())
    }

    /// The next stop after `note` that is not done, then any before it. `None` when every other stop is done.
    pub fn next_open(&self, note: usize, done: impl Fn(usize) -> bool) -> Option<usize> {
        let at = self.0.iter().position(|&index| index == note).map_or(0, |at| at + 1);
        self.0[at..]
            .iter()
            .chain(&self.0[..at.saturating_sub(1)])
            .copied()
            .find(|&index| !done(index))
    }

    pub fn all_done(&self, done: impl Fn(usize) -> bool) -> bool {
        !self.0.is_empty() && self.0.iter().all(|&note| done(note))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffpilot_core::guide::StopKind;
    use std::path::PathBuf;

    fn note(path: &str, folder: Option<&str>) -> Note {
        Note {
            path: path.into(),
            span: None,
            text: String::new(),
            critical: false,
            kind: StopKind::Check,
            check: None,
            node: None,
            folder: folder.map(PathBuf::from),
        }
    }

    fn stops() -> Stops {
        let notes = [
            note("a.rs", None),
            note("gone.rs", None),
            note("b.rs", None),
            note("shared.rs", Some("/lib")),
            note("a.rs", None),
        ];
        Stops::of(&notes, &["a.rs".into(), "b.rs".into()])
    }

    #[test]
    fn a_note_whose_file_left_the_diff_drops_out_and_the_others_keep_their_index() {
        let stops = stops();
        assert_eq!(stops.0, [0, 2, 3, 4]);
        assert_eq!(stops.number(2), Some(2));
        assert_eq!(stops.number(1), None);
    }

    #[test]
    fn next_and_previous_stop_at_the_edges() {
        let stops = stops();
        assert_eq!(stops.next(0), Some(2));
        assert_eq!(stops.next(4), None);
        assert_eq!(stops.previous(2), Some(0));
        assert_eq!(stops.previous(0), None);
    }

    #[test]
    fn the_entry_is_the_first_open_stop_or_the_first_stop() {
        let stops = stops();
        assert_eq!(stops.entry(|note| note == 0 || note == 2), Some(3));
        assert_eq!(stops.entry(|_| true), Some(0));
        assert_eq!(Stops::default().entry(|_| false), None);
    }

    #[test]
    fn the_next_open_stop_follows_the_cursor_then_wraps() {
        let stops = stops();
        assert_eq!(stops.next_open(0, |note| note == 0), Some(2));
        assert_eq!(stops.next_open(2, |note| note == 0 || note == 2 || note == 3), Some(4));
        assert_eq!(
            stops.next_open(4, |note| note != 2),
            Some(2),
            "wraps to the skipped stop"
        );
        assert_eq!(stops.next_open(4, |_| true), None);
    }

    #[test]
    fn a_path_is_done_when_every_stop_is_and_an_empty_path_never_is() {
        let stops = stops();
        assert!(stops.all_done(|_| true));
        assert!(!stops.all_done(|note| note != 3));
        assert!(!Stops::default().all_done(|_| true));
    }
}
