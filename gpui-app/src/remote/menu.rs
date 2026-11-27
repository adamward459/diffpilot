use super::RemoteStatus;
use super::rows::{Tone, Words, outcome_tone, overall_tone};
use diffpilot_ui_kit::Theme;
use diffpilot_ui_kit::components::{ChipTone, chip_label, status_dot, tag};
use gpui::{App, Div, Entity, ParentElement};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrChip {
    pub number: u64,
    pub tone: Option<Tone>,
}

#[derive(Clone)]
pub struct RemoteMenu {
    pub chip: Option<PrChip>,
    pub words: Option<Words>,
}

impl RemoteMenu {
    pub fn new(status: &Entity<RemoteStatus>, cx: &App) -> Self {
        let remote = status.read(cx);
        let chip = remote.branch_pr().map(|change| PrChip {
            number: change.number,
            tone: remote
                .checks()
                .and_then(|checks| overall_tone(checks.overall()))
                .or_else(|| change.checks.map(outcome_tone)),
        });
        Self {
            chip,
            words: remote.words(),
        }
    }

    pub fn chip_element(&self, theme: &Theme) -> Option<Div> {
        let chip = self.chip.as_ref()?;
        let number = chip.number;
        let reference = self.words?.reference(number);
        let dot = chip.tone.map(|tone| {
            tag(status_dot(tone.color(theme)), move || {
                format!("remote:pr-dot:{}", tone.name())
            })
        });
        Some(tag(
            chip_label(theme, reference, ChipTone::Blue).children(dot),
            move || format!("remote:pr-chip:{number}"),
        ))
    }
}
