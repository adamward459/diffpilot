use crate::claude_area::{ago, comments_of, state_glyph};
use crate::review::ReviewSession;
use crate::runs::now_ms;
use diffpilot_core::run::{LogKind, Run, RunKind, RunState};
use diffpilot_ui_kit::components::{Button, Look, bar_button, glyph_icon, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::FluentBuilder;
use gpui::{AnyElement, App, ClickEvent, IntoElement, ParentElement, Styled, Window, div, px};
use gpui_component::h_flex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Idle(String),
    Working(String),
    Finished(RunState, String),
}

impl Status {
    pub fn text(&self) -> &str {
        match self {
            Self::Idle(text) | Self::Working(text) | Self::Finished(_, text) => text,
        }
    }
}

pub fn status_of(session: &ReviewSession, now: i64) -> Status {
    let Some(run) = session.current() else {
        return Status::Idle(match session.last_run() {
            Some(last) => {
                let when = last
                    .finished_at_ms
                    .or(last.started_at_ms)
                    .map_or_else(String::new, |at| format!(" {}", ago(now, at)));
                format!("Claude · last {}{when}", last.kind.title().to_lowercase())
            }
            None => "Claude · no runs yet".into(),
        });
    };
    let title = run.kind.title();
    match run.state {
        RunState::Queued => Status::Working(format!("{title} · waiting for its turn")),
        RunState::Running => Status::Working(format!("{title} · {}", run.activity.as_deref().unwrap_or("starting"))),
        RunState::Done => Status::Finished(run.state, format!("{title} finished · {}", counts(session, run))),
        RunState::Failed => Status::Finished(run.state, format!("{title} failed")),
        RunState::Stopped => Status::Finished(run.state, format!("{title} stopped")),
    }
}

fn counts(session: &ReviewSession, run: &Run) -> String {
    let calls = run
        .log
        .iter()
        .filter(|entry| {
            matches!(
                entry.kind,
                LogKind::Read | LogKind::Search | LogKind::Edit | LogKind::Command | LogKind::Comment | LogKind::Tool
            )
        })
        .count();
    let plural = |count: usize, noun: &str| format!("{count} {noun}{}", if count == 1 { "" } else { "s" });
    let made = match run.kind {
        RunKind::Guide => plural(session.guide_chapters().unwrap_or(0), "chapter"),
        _ => plural(comments_of(run).len(), "comment"),
    };
    format!("{made} · {}", plural(calls, "tool call"))
}

pub struct Actions<T, L, A> {
    pub toggle: T,
    pub show_log: L,
    pub run_again: A,
}

pub fn status_row<T, L, A>(
    session: &ReviewSession,
    open: bool,
    generation: u32,
    theme: &Theme,
    actions: Actions<T, L, A>,
) -> AnyElement
where
    T: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    L: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    A: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    let status = status_of(session, now_ms());
    let icon = match &status {
        Status::Idle(_) => glyph_icon(Glyph::Sparkles, 14., theme.ui.muted).into_any_element(),
        Status::Working(_) => glyph_icon(Glyph::Sparkles, 14., theme.ui.claude).into_any_element(),
        Status::Finished(state, _) => {
            let (glyph, color) = state_glyph(*state, theme);
            glyph_icon(glyph, 14., color).into_any_element()
        }
    };
    let can_run_again = matches!(status, Status::Finished(..)) && session.can_run_again();
    let showing_log = !matches!(status, Status::Idle(_));
    let text = status.text().to_string();
    let toggle = tag(
        bar_button(
            theme,
            "claude-toggle",
            Glyph::PanelBottom,
            "Show Claude activity (⇧⌘Y)",
            true,
            open,
            actions.toggle,
        ),
        move || format!("claude:toggle:{generation}"),
    );
    h_flex()
        .gap(px(6.))
        .items_center()
        .child(toggle)
        .child(icon)
        .child(tag(div(), || format!("claude:status:{text}")).child(status.text().to_string()))
        .when_some(can_run_again.then_some(actions.run_again), |row, run_again| {
            row.child(
                tag(
                    Button::new("claude-status-again").look(Look::Ghost).label("Run Again"),
                    || "claude:status:run-again",
                )
                .on_click(run_again),
            )
        })
        .when_some(showing_log.then_some(actions.show_log), |row, show_log| {
            row.child(
                tag(
                    Button::new("claude-status-log").look(Look::Ghost).label("Show Log"),
                    || "claude:status:show-log",
                )
                .on_click(show_log),
            )
        })
        .into_any_element()
}
