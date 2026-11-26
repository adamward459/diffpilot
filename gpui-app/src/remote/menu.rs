use super::RemoteStatus;
use super::rows::{PrRow, Tone, overall_tone, pr_rows, rollup_tone};
use diffpilot_ui_kit::components::menu::{MenuItem, MenuRow, MenuSection};
use diffpilot_ui_kit::components::{ChipTone, chip_label, status_dot, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::{App, Div, Entity, ParentElement, Window};

/// The pull request of the checked-out branch, shown beside the branch name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrChip {
    pub number: u64,
    pub tone: Option<Tone>,
}

/// What the branch menu and the branch segment show of the remote. Empty when the repository is not on GitHub.
#[derive(Clone)]
pub struct RemoteMenu {
    pub chip: Option<PrChip>,
    pub rows: Vec<PrRow>,
    pub linked: bool,
    status: Entity<RemoteStatus>,
}

impl RemoteMenu {
    pub fn new(status: &Entity<RemoteStatus>, cx: &App) -> Self {
        let remote = status.read(cx);
        let branch_pr = remote.branch_pr();
        let chip = branch_pr.map(|pr| PrChip {
            number: pr.number,
            tone: remote
                .checks()
                .and_then(|checks| overall_tone(checks.overall()))
                .or_else(|| rollup_tone(pr.checks)),
        });
        Self {
            chip,
            rows: pr_rows(remote.pull_requests(), branch_pr.map(|pr| pr.number)),
            linked: remote.is_github(),
            status: status.clone(),
        }
    }

    pub fn chip_element(&self, theme: &Theme) -> Option<Div> {
        let chip = self.chip.as_ref()?;
        let number = chip.number;
        let dot = chip.tone.map(|tone| {
            tag(status_dot(tone.color(theme)), move || {
                format!("remote:pr-dot:{}", tone.name())
            })
        });
        Some(tag(
            chip_label(theme, format!("#{number}"), ChipTone::Blue).children(dot),
            move || format!("remote:pr-chip:{number}"),
        ))
    }

    pub fn sections(&self) -> Vec<MenuSection> {
        if !self.linked {
            return Vec::new();
        }
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
            MenuItem::new("Open on GitHub", move |_: &mut Window, cx: &mut App| {
                status.update(cx, |status, cx| status.open_on_github(cx));
            })
            .icon(Glyph::Github, None),
        );
        vec![
            MenuSection::titled("Pull requests", pull_requests),
            MenuSection::titled("Remote", vec![remote]),
        ]
    }
}
