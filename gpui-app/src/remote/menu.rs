use super::RemoteStatus;
use super::rows::{PrRow, Tone, Words, outcome_tone, overall_tone, pr_rows};
use diffpilot_core::comparison::Comparison;
use diffpilot_core::forge::pr_number;
use diffpilot_ui_kit::components::menu::{MenuItem, MenuRow, MenuSection};
use diffpilot_ui_kit::components::{ChipTone, chip_label, status_dot, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::{App, Div, Entity, ParentElement, Window};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrChip {
    pub number: u64,
    pub tone: Option<Tone>,
}

#[derive(Clone)]
pub struct RemoteMenu {
    pub chip: Option<PrChip>,
    pub rows: Vec<PrRow>,
    pub words: Option<Words>,
    status: Entity<RemoteStatus>,
}

impl RemoteMenu {
    pub fn new(status: &Entity<RemoteStatus>, comparison: Option<&Comparison>, cx: &App) -> Self {
        let remote = status.read(cx);
        let words = remote.words();
        let branch_pr = remote.branch_pr();
        let chip = branch_pr.map(|change| PrChip {
            number: change.number,
            tone: remote
                .checks()
                .and_then(|checks| overall_tone(checks.overall()))
                .or_else(|| change.checks.map(outcome_tone)),
        });
        let rows = words.map_or_else(Vec::new, |words| {
            pr_rows(
                remote.pull_requests(),
                match comparison.and_then(Comparison::branch_name) {
                    Some(branch) => pr_number(branch),
                    None => branch_pr.map(|change| change.number),
                },
                words,
            )
        });
        Self {
            chip,
            rows,
            words,
            status: status.clone(),
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

    pub fn sections(&self) -> Vec<MenuSection> {
        let Some(words) = self.words else {
            return Vec::new();
        };
        let pull_requests = self
            .rows
            .iter()
            .map(|row| {
                let (status, number) = (self.status.clone(), row.number);
                MenuRow::Item(
                    MenuItem::new(row.title.clone(), move |_: &mut Window, cx: &mut App| {
                        status.update(cx, |status, cx| status.review(number, cx));
                    })
                    .detail(row.detail.clone())
                    .icon(Glyph::GitPullRequest, None)
                    .checked(row.checked),
                )
            })
            .collect();
        let status = self.status.clone();
        let remote = MenuRow::Item(
            MenuItem::new(words.open_on_host(), move |_: &mut Window, cx: &mut App| {
                status.update(cx, |status, cx| status.open_on_host(cx));
            })
            .icon(Glyph::Github, None),
        );
        vec![
            MenuSection::titled(words.requests, pull_requests),
            MenuSection::titled("Remote", vec![remote]),
        ]
    }
}
