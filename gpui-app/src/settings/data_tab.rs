use super::page::{Load, Op, PageEvent, SettingsPage};
use crate::format::ago;
use crate::runs::{Runs, now_ms};
use diffpilot_core::store::{BranchData, DataScope, RepoData};
use diffpilot_ui_kit::components::{Button, Row, caption, grouped, icon_button, note, spinner, tag};
use diffpilot_ui_kit::{Glyph, Theme};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, ElementId, IntoElement, div, px};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub data: RepoData,
    pub missing: bool,
}

impl SettingsPage {
    pub(super) fn refresh_data(&mut self, cx: &mut Context<Self>) {
        if self.data == Load::Idle {
            self.data = Load::Loading;
        }
        let store = self.services.store.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    store.projects().map(|repos| {
                        repos
                            .into_iter()
                            .map(|data| Project {
                                missing: !Path::new(&data.repo).is_dir(),
                                data,
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(projects) => this.data = Load::Ready(projects),
                    Err(error) => {
                        this.data = Load::Ready(Vec::new());
                        this.data_op = Op::Failed(error.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn toggle_repo(&mut self, repo: &str, cx: &mut Context<Self>) {
        if !self.data_open.remove(repo) {
            self.data_open.insert(repo.to_string());
        }
        cx.notify();
    }

    pub(super) fn ask_delete(&mut self, scope: DataScope, cx: &mut Context<Self>) {
        if !self.data_op.running() {
            self.data_confirm = Some(scope);
            cx.notify();
        }
    }

    pub(super) fn cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.data_confirm = None;
        cx.notify();
    }

    pub(super) fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(scope) = self.data_confirm.take() else { return };
        if self.data_op.running() {
            return;
        }
        self.data_op = Op::Running;
        if let Some(runs) = cx.try_global::<Runs>() {
            runs.engine().stop_runs(&scope);
        }
        let store = self.services.store.clone();
        let target = scope.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { store.delete(&target) }).await;
            this.update(cx, |this, cx| {
                this.data_op = match result {
                    Ok(()) => {
                        cx.emit(PageEvent::DataDeleted(scope));
                        Op::Idle
                    }
                    Err(error) => Op::Failed(error.to_string()),
                };
                this.refresh_data(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

pub fn render(window: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> Div {
    let intro = caption(
        t,
        "Diffpilot keeps review threads, comments, Claude runs, guides and viewed marks for each branch. \
         Deleting them never changes your repos.",
    );
    let body = match &window.data {
        Load::Ready(projects) if projects.is_empty() => tag(
            caption(
                t,
                "Nothing is stored yet. Data appears here after you comment on or review a branch.",
            ),
            || "data:empty",
        )
        .into_any_element(),
        Load::Ready(projects) => {
            let now = now_ms();
            let rows: Vec<AnyElement> = projects
                .iter()
                .flat_map(|project| repo_rows(window, t, cx, project, now))
                .collect();
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(grouped(t, rows))
                .child(grouped(t, vec![delete_all_row(window, t, cx)]))
                .into_any_element()
        }
        _ => div()
            .flex()
            .items_center()
            .gap(px(5.))
            .text_size(diffpilot_ui_kit::text_px(12.))
            .text_color(t.ui.muted)
            .child(spinner("data-load", 12., t.ui.muted))
            .child("Loading…")
            .into_any_element(),
    };
    let mut content = div().flex().flex_col().gap(px(10.)).child(intro);
    if let Some(message) = window.data_op.failure() {
        content = content.child(note(t, t.danger, message.to_string()));
    }
    content.child(body)
}

fn id(text: String) -> ElementId {
    ElementId::Name(text.into())
}

fn plural(count: u32, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn describe(counts: [u32; 5]) -> String {
    const NOUNS: [&str; 5] = ["thread", "comment", "run", "guide", "viewed file"];
    counts
        .into_iter()
        .zip(NOUNS)
        .filter(|(count, _)| *count > 0)
        .map(|(count, noun)| plural(count, noun))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn branch_counts(branch: &BranchData) -> [u32; 5] {
    [
        branch.threads,
        branch.comments,
        branch.runs,
        branch.guides,
        branch.viewed_files,
    ]
}

fn repo_counts(repo: &RepoData) -> [u32; 5] {
    [
        repo.total(|branch| branch.threads),
        repo.total(|branch| branch.comments),
        repo.total(|branch| branch.runs),
        repo.total(|branch| branch.guides),
        repo.total(|branch| branch.viewed_files),
    ]
}

fn warning(t: &Theme, text: &'static str) -> Div {
    div()
        .text_size(diffpilot_ui_kit::text_px(13.))
        .line_height(px(18.))
        .text_color(t.danger)
        .child(text)
}

fn delete_control(
    window: &SettingsPage,
    t: &Theme,
    cx: &mut Context<SettingsPage>,
    scope: DataScope,
    key: &str,
) -> Div {
    let controls = div().flex().items_center().gap(px(8.));
    if window.data_confirm.as_ref() == Some(&scope) {
        return controls
            .child(
                tag(
                    Button::new(id(format!("cancel-{key}"))).label("Cancel"),
                    || "data:cancel",
                )
                .on_click(cx.listener(|this, _, _, cx| this.cancel_delete(cx))),
            )
            .child(
                tag(
                    Button::new(id(format!("confirm-{key}")))
                        .label("Delete")
                        .text_color(t.danger),
                    || "data:confirm",
                )
                .on_click(cx.listener(|this, _, _, cx| this.confirm_delete(cx))),
            );
    }
    controls.child(
        tag(
            Button::new(id(format!("delete-{key}")))
                .label("Delete")
                .disabled(window.data_op.running()),
            || format!("data:delete:{key}"),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.ask_delete(scope.clone(), cx))),
    )
}

fn repo_rows(
    window: &SettingsPage,
    t: &Theme,
    cx: &mut Context<SettingsPage>,
    project: &Project,
    now: i64,
) -> Vec<AnyElement> {
    let repo = project.data.repo.clone();
    let name = Path::new(&repo)
        .file_name()
        .map_or_else(|| repo.clone(), |name| name.to_string_lossy().into_owned());
    let counts = describe(repo_counts(&project.data));
    let open = window.data_open.contains(&repo);
    let scope = DataScope::Repo(repo.clone());
    let sub = if window.data_confirm.as_ref() == Some(&scope) {
        warning(
            t,
            "Deletes the data of every branch of this repo. This cannot be undone.",
        )
    } else {
        caption(
            t,
            format!("{repo} · {counts} · {}", ago(now, project.data.last_activity_ms())),
        )
    };
    let toggle_repo = repo.clone();
    let control = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .when(project.missing, |this| {
            this.child(tag(
                div()
                    .text_size(diffpilot_ui_kit::text_px(12.))
                    .text_color(t.amber)
                    .child("Folder missing"),
                || format!("data:missing:{repo}"),
            ))
        })
        .child(delete_control(window, t, cx, scope, &format!("repo:{repo}")))
        .child(
            tag(
                icon_button(
                    id(format!("expand-{repo}")),
                    if open { Glyph::ChevronDown } else { Glyph::ChevronRight },
                    t.ui.muted,
                ),
                || format!("data:expand:{repo}"),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_repo(&toggle_repo, cx))),
        );
    let header = tag(div().id(id(format!("repo-{repo}"))), || {
        format!("data:repo:{repo}:{counts}")
    })
    .child(
        Row::new(t, name)
            .leading(if project.missing {
                Glyph::TriangleAlert
            } else {
                Glyph::Folder
            })
            .subtitle(sub)
            .control(control),
    );
    let mut rows = vec![header.into_any_element()];
    if open {
        for branch in &project.data.branches {
            rows.push(branch_row(window, t, cx, &repo, branch, now));
        }
    }
    rows
}

fn branch_row(
    window: &SettingsPage,
    t: &Theme,
    cx: &mut Context<SettingsPage>,
    repo: &str,
    branch: &BranchData,
    now: i64,
) -> AnyElement {
    let counts = describe(branch_counts(branch));
    let scope = DataScope::Branch {
        repo: repo.to_string(),
        branch: branch.branch.clone(),
    };
    let sub = if window.data_confirm.as_ref() == Some(&scope) {
        warning(t, "Deletes the data of this branch. This cannot be undone.")
    } else {
        caption(t, format!("{counts} · {}", ago(now, branch.last_activity_ms)))
    };
    let key = format!("branch:{repo}:{}", branch.branch);
    let control = delete_control(window, t, cx, scope, &key);
    div()
        .pl(px(28.))
        .child(
            tag(div().id(id(format!("branch-{repo}-{}", branch.branch))), || {
                format!("data:branch:{repo}:{}:{counts}", branch.branch)
            })
            .child(
                Row::new(t, branch.branch.clone())
                    .leading(Glyph::GitBranch)
                    .subtitle(sub)
                    .control(control),
            ),
        )
        .into_any_element()
}

fn delete_all_row(window: &SettingsPage, t: &Theme, cx: &mut Context<SettingsPage>) -> AnyElement {
    let sub = if window.data_confirm == Some(DataScope::All) {
        warning(t, "Deletes everything listed above. This cannot be undone.")
    } else {
        caption(t, "Removes the data of every repo.")
    };
    Row::new(t, "Delete all data")
        .subtitle(sub)
        .control(delete_control(window, t, cx, DataScope::All, "all"))
        .into_any_element()
}
