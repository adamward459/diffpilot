use crate::anchor::{self, Placement};
use crate::diff::FileDiff;
use crate::review::FindingMeta;
use crate::store::{NewComment, NewThread, RunId, SessionId, SessionKey, Store, StoreError, Written};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const USER_NAME: &str = "You";
pub const CLAUDE_NAME: &str = "Claude";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Old,
    New,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ThreadAnchor {
    File { path: String },
    Line { path: String, side: Side, line: u32 },
    Change,
}

impl ThreadAnchor {
    pub fn file(path: &str) -> Self {
        Self::File { path: path.to_string() }
    }

    pub fn line(path: &str, side: Side, line: u32) -> Self {
        Self::Line {
            path: path.to_string(),
            side,
            line,
        }
    }

    pub fn path(&self) -> &str {
        match self {
            Self::File { path } | Self::Line { path, .. } => path,
            Self::Change => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadStatus {
    #[default]
    Open,
    Resolved,
    Dismissed,
}

impl ThreadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
            Self::Dismissed => "dismissed",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "resolved" => Self::Resolved,
            "dismissed" => Self::Dismissed,
            _ => Self::Open,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentSource {
    #[default]
    User,
    Claude,
    Terminal,
}

impl CommentSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Claude => "claude",
            Self::Terminal => "terminal",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "claude" => Self::Claude,
            "terminal" => Self::Terminal,
            _ => Self::User,
        }
    }
}

/// Claude reported this thread as done in `run`. The thread stays open until the reviewer resolves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Addressed {
    pub run: RunId,
    pub summary: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadComment {
    pub id: u32,
    pub author: String,
    pub body: String,
    pub is_streaming: bool,
    #[serde(default)]
    pub source: CommentSource,
    #[serde(default)]
    pub pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewThread {
    pub id: ThreadId,
    pub anchor: ThreadAnchor,
    pub comments: Vec<ThreadComment>,
    pub finding: Option<FindingMeta>,
    #[serde(default)]
    pub end_line: Option<u32>,
    #[serde(default)]
    pub anchor_text: Option<String>,
    #[serde(default)]
    pub status: ThreadStatus,
    #[serde(default)]
    pub addressed: Option<Addressed>,
}

impl ReviewThread {
    /// The comments a run may read: pending ones stay private until the review is finished.
    pub fn sent_comments(&self) -> impl Iterator<Item = &ThreadComment> {
        self.comments.iter().filter(|comment| !comment.pending)
    }
}

/// The threads of one review (repo + branch + base), kept in memory and written through to sqlite.
///
/// `new()` keeps them in a private in-memory database. `open()` shares a `Store` with the run engine and the MCP
/// bridge, which write threads too; call `refresh()` after they report a change.
#[derive(Debug)]
pub struct ThreadStore {
    store: Arc<Store>,
    session: SessionId,
    threads: BTreeMap<ThreadId, ReviewThread>,
    seen: i64,
    error: Option<String>,
}

impl Default for ThreadStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadStore {
    pub fn new() -> Self {
        let store = Arc::new(Store::open_in_memory().expect("an in-memory sqlite database opens"));
        Self::open(store, &SessionKey::new("", "", "")).expect("a new in-memory database has a session")
    }

    pub fn open(store: Arc<Store>, key: &SessionKey) -> Result<Self, StoreError> {
        let session = store.session(key)?;
        store.end_stale_streams(session)?;
        let mut threads = Self {
            store,
            session,
            threads: BTreeMap::new(),
            seen: -1,
            error: None,
        };
        threads.reload()?;
        Ok(threads)
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn session(&self) -> SessionId {
        self.session
    }

    /// The last write that did not reach the database. The change is kept in memory until the next `refresh`.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Reloads when another writer changed this review. Returns whether anything was reloaded.
    pub fn refresh(&mut self) -> bool {
        match self.store.revision(self.session) {
            Ok(revision) if revision != self.seen => self.reload().is_ok(),
            _ => false,
        }
    }

    fn reload(&mut self) -> Result<(), StoreError> {
        let loaded = self.store.load_threads(self.session)?;
        self.threads = loaded.value.into_iter().map(|thread| (thread.id, thread)).collect();
        self.seen = loaded.revision;
        Ok(())
    }

    pub fn threads(&self) -> impl Iterator<Item = &ReviewThread> {
        self.threads.values()
    }

    pub fn get(&self, id: ThreadId) -> Option<&ReviewThread> {
        self.threads.get(&id)
    }

    pub fn len(&self) -> usize {
        self.threads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    /// Reuses the thread already open on `anchor`.
    pub fn create(&mut self, anchor: ThreadAnchor) -> ReviewThread {
        self.create_range(anchor, None, None)
    }

    /// Like `create`, for a comment on lines `anchor.line..=end_line`. `anchor_text` is those lines' text,
    /// which lets the thread follow its code when lines move.
    pub fn create_range(
        &mut self,
        anchor: ThreadAnchor,
        end_line: Option<u32>,
        anchor_text: Option<String>,
    ) -> ReviewThread {
        if let Some(existing) = self
            .threads
            .values()
            .find(|thread| thread.anchor == anchor && thread.status == ThreadStatus::Open)
        {
            return existing.clone();
        }
        self.insert(NewThread {
            anchor,
            end_line,
            anchor_text,
            finding: None,
            first_comment: None,
        })
    }

    pub fn add_finding(&mut self, anchor: ThreadAnchor, finding: Option<FindingMeta>, body: &str) -> ReviewThread {
        self.insert(NewThread {
            anchor,
            end_line: None,
            anchor_text: None,
            finding,
            first_comment: Some(comment(CLAUDE_NAME, CommentSource::Claude, body, false)),
        })
    }

    pub fn post(&mut self, id: ThreadId, text: &str) -> Option<ReviewThread> {
        self.get(id)?;
        let written = self
            .store
            .insert_comment(id, &comment(USER_NAME, CommentSource::User, text, false));
        self.adopt(written)
    }

    /// Only the reviewer's own comments can be edited.
    pub fn edit(&mut self, id: ThreadId, comment: u32, text: &str) -> Option<ReviewThread> {
        let own = self
            .get(id)?
            .comments
            .iter()
            .any(|c| c.id == comment && c.source == CommentSource::User && !c.is_streaming);
        if !own {
            return None;
        }
        let written = self.store.update_comment(id, comment, text);
        self.adopt(written)
    }

    pub fn begin_claude_reply(&mut self, id: ThreadId) -> Option<ReviewThread> {
        if self.get(id)?.comments.iter().any(|comment| comment.is_streaming) {
            return None;
        }
        let written = self
            .store
            .insert_comment(id, &comment(CLAUDE_NAME, CommentSource::Claude, "", true));
        self.adopt(written)
    }

    pub fn append_claude(&mut self, id: ThreadId, chunk: &str, done: bool) -> Option<ReviewThread> {
        self.get(id)?;
        let written = self.store.append_streaming(id, chunk, done);
        self.adopt(written)
    }

    pub fn remove(&mut self, id: ThreadId) {
        match self.store.delete_thread(id) {
            Ok(Some(revision)) => self.synced(revision),
            Ok(None) => {}
            Err(error) => self.error = Some(error.to_string()),
        }
        self.threads.remove(&id);
    }

    pub fn set_status(&mut self, id: ThreadId, status: ThreadStatus) -> Option<ReviewThread> {
        let written = self.store.set_status(id, status);
        self.adopt(written)
    }

    /// Clears "addressed by Claude", for example when the reviewer is not satisfied and reopens the thread.
    pub fn clear_addressed(&mut self, id: ThreadId) -> Option<ReviewThread> {
        let written = self.store.clear_addressed(id);
        self.adopt(written)
    }

    /// Makes the pending comments of this review visible to runs.
    pub fn finish_pending(&mut self) -> usize {
        match self.store.finish_pending(self.session) {
            Ok(count) => {
                if count > 0 {
                    self.refresh();
                }
                count
            }
            Err(error) => {
                self.error = Some(error.to_string());
                0
            }
        }
    }

    /// Places every line thread in `files`, the current diff. A thread whose code moved follows it. Threads that
    /// have no `anchor_text` yet get it from the diff. Returns the placement of each line and file thread.
    pub fn reanchor(&mut self, files: &[FileDiff]) -> BTreeMap<ThreadId, Placement> {
        let mut placements = BTreeMap::new();
        let ids: Vec<ThreadId> = self.threads.keys().copied().collect();
        for id in ids {
            let Some(thread) = self.threads.get(&id) else { continue };
            if matches!(thread.anchor, ThreadAnchor::Change) {
                continue;
            }
            if thread.anchor_text.is_none()
                && let Some(text) = anchor::text_of(thread, files)
            {
                let written = self.store.set_anchor_text(id, Some(&text));
                self.adopt(written);
            }
            let Some(thread) = self.threads.get(&id) else { continue };
            let placement = anchor::place(thread, files);
            if let Placement::Moved { line, end_line } = placement {
                let written = self.store.move_thread(id, line, end_line);
                self.adopt(written);
            }
            placements.insert(id, placement);
        }
        placements
    }

    /// Thread count per file path. Threads on the whole change count under `""`.
    pub fn comment_counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for thread in self.threads.values() {
            *counts.entry(thread.anchor.path().to_string()).or_insert(0) += 1;
        }
        counts
    }

    fn insert(&mut self, new: NewThread) -> ReviewThread {
        match self.store.insert_thread(self.session, &new) {
            Ok(written) => self.adopt(Ok(Some(written))).expect("the thread was just written"),
            Err(error) => {
                self.error = Some(error.to_string());
                detached(new)
            }
        }
    }

    fn adopt(&mut self, written: Result<Option<Written<ReviewThread>>, StoreError>) -> Option<ReviewThread> {
        match written {
            Ok(Some(written)) => {
                let thread = written.value;
                self.threads.insert(thread.id, thread.clone());
                self.synced(written.revision);
                Some(thread)
            }
            Ok(None) => None,
            Err(error) => {
                self.error = Some(error.to_string());
                None
            }
        }
    }

    /// After our own write: skip the reload when nobody else wrote in between.
    fn synced(&mut self, revision: i64) {
        if revision == self.seen + 1 {
            self.seen = revision;
        } else {
            let _ = self.reload();
        }
    }
}

fn comment(author: &str, source: CommentSource, body: &str, streaming: bool) -> NewComment {
    NewComment {
        author: author.into(),
        source,
        body: body.into(),
        streaming,
        pending: false,
    }
}

fn detached(new: NewThread) -> ReviewThread {
    ReviewThread {
        id: ThreadId(0),
        anchor: new.anchor,
        comments: Vec::new(),
        finding: new.finding,
        end_line: new.end_line,
        anchor_text: new.anchor_text,
        status: ThreadStatus::Open,
        addressed: None,
    }
}

/// True when `text` mentions `@claude` as a whole word, not inside an email or a longer name.
pub fn mentions_claude(text: &str) -> bool {
    const NAME: &str = "@claude";
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.char_indices().any(|(start, c)| {
        if c != '@' {
            return false;
        }
        let end = start + NAME.len();
        let Some(candidate) = text.get(start..end) else {
            return false;
        };
        candidate.eq_ignore_ascii_case(NAME)
            && !text[..start]
                .chars()
                .next_back()
                .is_some_and(|before| is_word(before) || before == '@')
            && !text[end..].chars().next().is_some_and(is_word)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor() -> ThreadAnchor {
        ThreadAnchor::line("a.ts", Side::New, 7)
    }

    #[test]
    fn create_reuses_a_thread_on_the_same_anchor() {
        let mut store = ThreadStore::new();
        let first = store.create(anchor());
        let again = store.create(anchor());
        let other = store.create(ThreadAnchor::file("a.ts"));
        assert_eq!(first.id, again.id);
        assert_ne!(other.id, first.id);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn post_adds_a_user_comment() {
        let mut store = ThreadStore::new();
        let id = store.create(anchor()).id;
        let thread = store.post(id, "hi").unwrap();
        let authors: Vec<_> = thread.comments.iter().map(|c| c.author.as_str()).collect();
        let bodies: Vec<_> = thread.comments.iter().map(|c| c.body.as_str()).collect();
        assert_eq!(authors, ["You"]);
        assert_eq!(bodies, ["hi"]);
        assert_eq!(store.post(ThreadId(99), "x"), None);
    }

    #[test]
    fn edit_changes_the_reviewers_comment_only() {
        let mut store = ThreadStore::new();
        let id = store.add_finding(anchor(), None, "Claude found this.").id;
        store.post(id, "Frist note");
        let thread = store.edit(id, 2, "First note").unwrap();
        assert_eq!(thread.comments[1].body, "First note");
        assert_eq!(store.edit(id, 1, "Not mine"), None);
        assert_eq!(store.get(id).unwrap().comments[0].body, "Claude found this.");
        assert_eq!(store.edit(ThreadId(99), 1, "x"), None);
    }

    #[test]
    fn claude_reply_streams_into_one_comment() {
        let mut store = ThreadStore::new();
        let id = store.create(anchor()).id;
        store.post(id, "@claude look");
        let started = store.begin_claude_reply(id).unwrap();
        assert!(started.comments.last().unwrap().is_streaming);
        store.append_claude(id, "one ", false);
        let finished = store.append_claude(id, "two", true).unwrap();
        assert_eq!(finished.comments.last().unwrap().body, "one two");
        assert!(!finished.comments.last().unwrap().is_streaming);
        assert_eq!(finished.comments.len(), 2);
    }

    #[test]
    fn a_second_reply_cannot_start_while_one_streams() {
        let mut store = ThreadStore::new();
        let id = store.create(anchor()).id;
        store.begin_claude_reply(id);
        assert_eq!(store.begin_claude_reply(id), None);
    }

    #[test]
    fn append_without_a_streaming_reply_does_nothing() {
        let mut store = ThreadStore::new();
        let id = store.create(anchor()).id;
        assert_eq!(store.append_claude(id, "x", false), None);
    }

    #[test]
    fn remove_deletes_the_thread() {
        let mut store = ThreadStore::new();
        let id = store.create(anchor()).id;
        store.remove(id);
        assert!(store.is_empty());
        assert_ne!(store.create(anchor()).id, id);
    }

    #[test]
    fn findings_never_share_a_thread_even_on_the_same_anchor() {
        let mut store = ThreadStore::new();
        let a = store.add_finding(ThreadAnchor::Change, None, "a");
        let b = store.add_finding(ThreadAnchor::Change, None, "b");
        assert_ne!(a.id, b.id);
        assert_eq!(store.len(), 2);
    }

    fn file_store() -> (tempfile::TempDir, Arc<Store>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&dir.path().join("review.sqlite3")).unwrap());
        (dir, store)
    }

    fn key() -> SessionKey {
        SessionKey::new("/r", "feature", "main")
    }

    #[test]
    fn threads_and_comments_survive_a_reopen() {
        let (dir, store) = file_store();
        let id = {
            let mut threads = ThreadStore::open(store, &key()).unwrap();
            let id = threads.create_range(anchor(), Some(9), Some("a\nb".into())).id;
            threads.post(id, "first");
            threads.begin_claude_reply(id);
            threads.append_claude(id, "answer", true);
            threads.add_finding(
                ThreadAnchor::Change,
                Some(FindingMeta {
                    severity: crate::review::Severity::Nit,
                    title: "Name".into(),
                    location: Some("a.ts:1".into()),
                }),
                "Rename it.",
            );
            id
        };
        let reopened = Arc::new(Store::open(&dir.path().join("review.sqlite3")).unwrap());
        let threads = ThreadStore::open(reopened, &key()).unwrap();
        assert_eq!(threads.len(), 2);
        let thread = threads.get(id).unwrap();
        assert_eq!(thread.anchor, anchor());
        assert_eq!(
            (thread.end_line, thread.anchor_text.as_deref()),
            (Some(9), Some("a\nb"))
        );
        let bodies: Vec<_> = thread
            .comments
            .iter()
            .map(|c| (c.author.as_str(), c.body.as_str()))
            .collect();
        assert_eq!(bodies, [("You", "first"), ("Claude", "answer")]);
        assert!(thread.comments.iter().all(|comment| !comment.is_streaming));
        let finding = threads.threads().find_map(|thread| thread.finding.clone()).unwrap();
        assert_eq!(
            (finding.title.as_str(), finding.location.as_deref()),
            ("Name", Some("a.ts:1"))
        );
    }

    #[test]
    fn other_reviews_do_not_see_these_threads() {
        let (_dir, store) = file_store();
        let mut here = ThreadStore::open(store.clone(), &key()).unwrap();
        here.create(anchor());
        let other_base = ThreadStore::open(store.clone(), &SessionKey::new("/r", "feature", "develop")).unwrap();
        let other_branch = ThreadStore::open(store, &SessionKey::new("/r", "topic", "main")).unwrap();
        assert!(other_base.is_empty() && other_branch.is_empty());
    }

    #[test]
    fn a_reply_that_was_streaming_when_the_app_quit_is_ended_on_reopen() {
        let (_dir, store) = file_store();
        let mut threads = ThreadStore::open(store.clone(), &key()).unwrap();
        let id = threads.create(anchor()).id;
        threads.begin_claude_reply(id);
        threads.append_claude(id, "half", false);
        let again = ThreadStore::open(store, &key()).unwrap();
        let comment = &again.get(id).unwrap().comments[0];
        assert_eq!((comment.body.as_str(), comment.is_streaming), ("half", false));
    }

    #[test]
    fn refresh_picks_up_a_write_from_another_handle() {
        let (_dir, store) = file_store();
        let mut mine = ThreadStore::open(store.clone(), &key()).unwrap();
        let mut theirs = ThreadStore::open(store, &key()).unwrap();
        let id = theirs.create(anchor()).id;
        theirs.post(id, "from the bridge");
        assert!(mine.is_empty());
        assert!(mine.refresh());
        assert_eq!(mine.get(id).unwrap().comments.len(), 1);
        assert!(!mine.refresh());
        mine.post(id, "mine");
        assert!(!mine.refresh());
        assert!(theirs.refresh());
        assert_eq!(theirs.get(id).unwrap().comments.len(), 2);
    }

    #[test]
    fn a_write_after_a_foreign_write_reloads_the_foreign_one() {
        let (_dir, store) = file_store();
        let mut mine = ThreadStore::open(store.clone(), &key()).unwrap();
        let mut theirs = ThreadStore::open(store, &key()).unwrap();
        let theirs_id = theirs.create(ThreadAnchor::file("b.ts")).id;
        let mine_id = mine.create(anchor()).id;
        assert!(mine.get(theirs_id).is_some() && mine.get(mine_id).is_some());
    }

    #[test]
    fn status_and_addressed_are_saved_and_a_reopened_thread_is_not_reused() {
        let (_dir, store) = file_store();
        let mut threads = ThreadStore::open(store.clone(), &key()).unwrap();
        let id = threads.create(anchor()).id;
        let addressed = Addressed {
            run: RunId(4),
            summary: "Fixed: null check.".into(),
            files: vec!["a.ts".into()],
        };
        threads.store().mark_addressed(id, &addressed).unwrap();
        threads.refresh();
        assert_eq!(threads.get(id).unwrap().addressed, Some(addressed));
        threads.set_status(id, ThreadStatus::Resolved);
        assert_ne!(threads.create(anchor()).id, id);
        let again = ThreadStore::open(store, &key()).unwrap();
        assert_eq!(again.get(id).unwrap().status, ThreadStatus::Resolved);
        assert!(again.get(id).unwrap().addressed.is_some());
    }

    #[test]
    fn pending_comments_stay_private_until_the_review_is_finished() {
        let (_dir, store) = file_store();
        let mut threads = ThreadStore::open(store.clone(), &key()).unwrap();
        let id = threads.create(anchor()).id;
        store
            .insert_comment(
                id,
                &NewComment {
                    author: USER_NAME.into(),
                    source: CommentSource::User,
                    body: "draft".into(),
                    streaming: false,
                    pending: true,
                },
            )
            .unwrap();
        threads.post(id, "sent");
        threads.refresh();
        let thread = threads.get(id).unwrap();
        assert_eq!(thread.comments.len(), 2);
        assert_eq!(
            thread.sent_comments().map(|c| c.body.as_str()).collect::<Vec<_>>(),
            ["sent"]
        );
        assert_eq!(threads.finish_pending(), 1);
        assert_eq!(threads.get(id).unwrap().sent_comments().count(), 2);
    }

    #[test]
    fn reanchoring_moves_a_thread_and_the_move_is_saved() {
        use crate::changes::FileStatus;
        use crate::diff::{DiffLine, Hunk, LineKind};
        let line = |kind, new, text: &str| DiffLine {
            kind,
            old: None,
            new: Some(new),
            text: text.into(),
        };
        let mut file = FileDiff::new("a.ts", FileStatus::Modified);
        file.hunks.push(Hunk {
            header: "@@".into(),
            lines: vec![line(LineKind::Added, 7, "let x = 1;")],
        });
        let before = [file.clone()];
        file.hunks[0].lines = vec![
            line(LineKind::Added, 7, "// top"),
            line(LineKind::Added, 8, "let x = 1;"),
        ];
        let after = [file];

        let (_dir, store) = file_store();
        let mut threads = ThreadStore::open(store.clone(), &key()).unwrap();
        let id = threads.create(ThreadAnchor::line("a.ts", Side::New, 7)).id;
        let first = threads.reanchor(&before);
        assert_eq!(first[&id], Placement::Current);
        assert_eq!(threads.get(id).unwrap().anchor_text.as_deref(), Some("let x = 1;"));

        let second = threads.reanchor(&after);
        assert_eq!(
            second[&id],
            Placement::Moved {
                line: 8,
                end_line: None
            }
        );
        let again = ThreadStore::open(store, &key()).unwrap();
        assert_eq!(again.get(id).unwrap().anchor, ThreadAnchor::line("a.ts", Side::New, 8));

        let gone = threads.reanchor(&[]);
        assert_eq!(gone[&id], Placement::FileGone);
    }

    #[test]
    fn detects_mentions() {
        let cases = [
            ("@claude check this", true),
            ("hey @Claude, look", true),
            ("a\n@claude", true),
            ("mail me@claude.com", false),
            ("@claudette", false),
            ("no mention", false),
            ("@@claude", false),
            ("first @claudette then @claude", true),
        ];
        for (text, expected) in cases {
            assert_eq!(mentions_claude(text), expected, "{text}");
        }
    }

    #[test]
    fn counts_threads_per_file() {
        let mut store = ThreadStore::new();
        store.create(ThreadAnchor::file("a.ts"));
        store.create(ThreadAnchor::line("a.ts", Side::New, 3));
        store.create(ThreadAnchor::file("b.ts"));
        assert_eq!(
            store.comment_counts(),
            BTreeMap::from([("a.ts".to_string(), 2), ("b.ts".to_string(), 1)])
        );
    }

    #[test]
    fn threads_round_trip_through_json() {
        let thread = ReviewThread {
            id: ThreadId(3),
            anchor: anchor(),
            comments: vec![ThreadComment {
                id: 1,
                author: "You".into(),
                body: "x".into(),
                is_streaming: false,
                source: CommentSource::User,
                pending: false,
            }],
            finding: None,
            end_line: None,
            anchor_text: None,
            status: ThreadStatus::Open,
            addressed: None,
        };
        let json = serde_json::to_value(&thread).unwrap();
        assert_eq!(serde_json::from_value::<ReviewThread>(json.clone()).unwrap(), thread);
        assert_eq!(json["anchor"]["kind"], "line");
        assert_eq!(json["anchor"]["side"], "new");
    }

    #[test]
    fn the_change_anchor_has_no_path_in_json() {
        let json = serde_json::to_value(ThreadAnchor::Change).unwrap();
        assert_eq!(json["kind"], "change");
        assert!(json.get("path").is_none());
        assert_eq!(
            serde_json::from_value::<ThreadAnchor>(json).unwrap(),
            ThreadAnchor::Change
        );
        let line = ThreadAnchor::line("a", Side::Old, 3);
        let back = serde_json::from_value::<ThreadAnchor>(serde_json::to_value(&line).unwrap());
        assert_eq!(back.unwrap(), line);
    }
}
