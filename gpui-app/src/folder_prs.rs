use crate::settings;
use diffpilot_core::forge::{self, Access, ChangeRequest, Linked};
use gpui::{App, AppContext as _, Context, Entity, Global, Task};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrList {
    Loading(Option<Vec<ChangeRequest>>),
    Loaded(Vec<ChangeRequest>),
    NoRemote,
    Failed(String),
}

impl PrList {
    pub fn requests(&self) -> &[ChangeRequest] {
        match self {
            PrList::Loading(Some(requests)) | PrList::Loaded(requests) => requests,
            PrList::Loading(None) | PrList::NoRemote | PrList::Failed(_) => &[],
        }
    }

    pub fn waiting_for_you(&self) -> usize {
        waiting_for_you(self.requests())
    }

    pub fn badge(&self) -> Option<String> {
        match self {
            PrList::Loading(None) => None,
            PrList::Loading(Some(requests)) | PrList::Loaded(requests) => Some(badge_text(requests)),
            PrList::NoRemote => Some("no remote".into()),
            PrList::Failed(_) => Some("offline".into()),
        }
    }
}

fn waiting_for_you(requests: &[ChangeRequest]) -> usize {
    requests.iter().filter(|request| request.requested_from_you).count()
}

fn badge_text(requests: &[ChangeRequest]) -> String {
    match (waiting_for_you(requests), requests.len()) {
        (0, 1) => "1 PR".into(),
        (0, count) => format!("{count} PRs"),
        (1, _) => "1 needs you".into(),
        (count, _) => format!("{count} need you"),
    }
}

#[derive(Default)]
pub struct FolderPrs {
    lists: HashMap<PathBuf, PrList>,
    connections: HashMap<PathBuf, Linked>,
    tasks: HashMap<PathBuf, Task<()>>,
}

struct Shared(Entity<FolderPrs>);

impl Global for Shared {}

impl FolderPrs {
    pub fn shared(cx: &mut App) -> Entity<Self> {
        if let Some(shared) = cx.try_global::<Shared>() {
            return shared.0.clone();
        }
        let entity = cx.new(|_| Self::default());
        cx.set_global(Shared(entity.clone()));
        entity
    }

    pub fn list(&self, folder: &Path) -> Option<&PrList> {
        self.lists.get(folder)
    }

    pub fn connection(&self, folder: &Path) -> Option<Linked> {
        self.connections.get(folder).cloned()
    }

    pub fn refresh(&mut self, folders: &[PathBuf], cx: &mut Context<Self>) {
        let services = settings::services(cx);
        for folder in folders {
            let kept = match self.lists.get(folder) {
                Some(PrList::Loaded(requests) | PrList::Loading(Some(requests))) => Some(requests.clone()),
                _ => None,
            };
            let Some(services) = services.clone() else {
                self.lists.insert(folder.clone(), PrList::NoRemote);
                continue;
            };
            self.lists.insert(folder.clone(), PrList::Loading(kept));
            let repo = folder.clone();
            let work = cx.background_spawn(async move {
                let access = Access {
                    http: services.http.clone(),
                    credentials: &services.credentials,
                    github_host: &services.host,
                };
                let link = forge::connect(&repo, &access).map_err(|_| None::<String>)?;
                let requests = link.forge.change_requests().map_err(|error| Some(error.to_string()))?;
                Ok::<_, Option<String>>((link, requests))
            });
            let key = folder.clone();
            let task = cx.spawn(async move |this, cx| {
                let loaded = work.await;
                this.update(cx, |this, cx| {
                    let list = match loaded {
                        Ok((link, requests)) => {
                            this.connections.insert(key.clone(), link);
                            PrList::Loaded(requests)
                        }
                        Err(None) => {
                            this.connections.remove(&key);
                            PrList::NoRemote
                        }
                        Err(Some(message)) => PrList::Failed(message),
                    };
                    this.lists.insert(key, list);
                    cx.notify();
                })
                .ok();
            });
            self.tasks.insert(folder.clone(), task);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffpilot_core::forge::{Outcome, Review};

    fn request(number: u64) -> ChangeRequest {
        ChangeRequest {
            number,
            title: "t".into(),
            url: String::new(),
            author: "a".into(),
            head_ref: "h".into(),
            base_ref: "main".into(),
            head_sha: String::new(),
            from_this_repo: true,
            review: Review::None,
            requested_from_you: false,
            checks: Some(Outcome::Passed),
        }
    }

    #[test]
    fn the_badge_counts_pull_requests_or_says_why_not() {
        assert_eq!(PrList::Loaded(vec![request(1)]).badge().as_deref(), Some("1 PR"));
        assert_eq!(
            PrList::Loaded(vec![request(1), request(2)]).badge().as_deref(),
            Some("2 PRs")
        );
        assert_eq!(PrList::Loaded(Vec::new()).badge().as_deref(), Some("0 PRs"));
        assert_eq!(PrList::NoRemote.badge().as_deref(), Some("no remote"));
        assert_eq!(PrList::Failed("x".into()).badge().as_deref(), Some("offline"));
    }

    #[test]
    fn the_badge_says_how_many_pull_requests_need_you() {
        let mine = |number| ChangeRequest {
            requested_from_you: true,
            ..request(number)
        };
        let one = PrList::Loaded(vec![request(1), mine(2)]);
        assert_eq!(one.badge().as_deref(), Some("1 needs you"));
        assert_eq!(one.waiting_for_you(), 1);
        let two = PrList::Loaded(vec![mine(1), mine(2), request(3)]);
        assert_eq!(two.badge().as_deref(), Some("2 need you"));
        assert_eq!(
            PrList::Loading(Some(vec![mine(1)])).badge().as_deref(),
            Some("1 needs you")
        );
        assert_eq!(PrList::NoRemote.waiting_for_you(), 0);
    }

    #[test]
    fn a_reload_keeps_showing_the_last_list() {
        assert_eq!(PrList::Loading(None).badge(), None);
        let reloading = PrList::Loading(Some(vec![request(1), request(2)]));
        assert_eq!(reloading.badge().as_deref(), Some("2 PRs"));
        assert_eq!(reloading.requests().len(), 2);
        assert!(PrList::NoRemote.requests().is_empty());
    }
}
