use crate::review::FindingMeta;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadComment {
    pub id: u32,
    pub author: String,
    pub body: String,
    pub is_streaming: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewThread {
    pub id: ThreadId,
    pub anchor: ThreadAnchor,
    pub comments: Vec<ThreadComment>,
    pub finding: Option<FindingMeta>,
}

#[derive(Debug, Default)]
pub struct ThreadStore {
    threads: BTreeMap<ThreadId, ReviewThread>,
    next_id: u32,
}

impl ThreadStore {
    pub fn new() -> Self {
        Self::default()
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
        if let Some(existing) = self.threads.values().find(|thread| thread.anchor == anchor) {
            return existing.clone();
        }
        self.insert(anchor, Vec::new(), None)
    }

    pub fn add_finding(&mut self, anchor: ThreadAnchor, finding: Option<FindingMeta>, body: &str) -> ReviewThread {
        let comment = ThreadComment {
            id: 1,
            author: CLAUDE_NAME.into(),
            body: body.into(),
            is_streaming: false,
        };
        self.insert(anchor, vec![comment], finding)
    }

    pub fn post(&mut self, id: ThreadId, text: &str) -> Option<ReviewThread> {
        self.append(id, USER_NAME, text, false)
    }

    pub fn begin_claude_reply(&mut self, id: ThreadId) -> Option<ReviewThread> {
        if self
            .threads
            .get(&id)?
            .comments
            .iter()
            .any(|comment| comment.is_streaming)
        {
            return None;
        }
        self.append(id, CLAUDE_NAME, "", true)
    }

    pub fn append_claude(&mut self, id: ThreadId, chunk: &str, done: bool) -> Option<ReviewThread> {
        let thread = self.threads.get_mut(&id)?;
        let comment = thread
            .comments
            .iter_mut()
            .rev()
            .find(|comment| comment.author == CLAUDE_NAME && comment.is_streaming)?;
        comment.body.push_str(chunk);
        if done {
            comment.is_streaming = false;
        }
        Some(thread.clone())
    }

    pub fn remove(&mut self, id: ThreadId) {
        self.threads.remove(&id);
    }

    /// Thread count per file path. Threads on the whole change count under `""`.
    pub fn comment_counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for thread in self.threads.values() {
            *counts.entry(thread.anchor.path().to_string()).or_insert(0) += 1;
        }
        counts
    }

    fn insert(
        &mut self,
        anchor: ThreadAnchor,
        comments: Vec<ThreadComment>,
        finding: Option<FindingMeta>,
    ) -> ReviewThread {
        self.next_id += 1;
        let thread = ReviewThread {
            id: ThreadId(self.next_id),
            anchor,
            comments,
            finding,
        };
        self.threads.insert(thread.id, thread.clone());
        thread
    }

    fn append(&mut self, id: ThreadId, author: &str, body: &str, is_streaming: bool) -> Option<ReviewThread> {
        let thread = self.threads.get_mut(&id)?;
        let comment_id = thread.comments.iter().map(|comment| comment.id).max().unwrap_or(0) + 1;
        thread.comments.push(ThreadComment {
            id: comment_id,
            author: author.into(),
            body: body.into(),
            is_streaming,
        });
        Some(thread.clone())
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
            }],
            finding: None,
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
