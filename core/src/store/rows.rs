use super::{Result, SessionId, Store, StoreError, now_ms};
use crate::review::{FindingMeta, Severity};
use crate::store::RunId;
use crate::threads::{
    Addressed, CommentSource, ReviewThread, Side, ThreadAnchor, ThreadComment, ThreadId, ThreadStatus,
};
use rusqlite::{Connection, OptionalExtension, Row};

/// A value together with the session revision the write produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written<T> {
    pub value: T,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewComment {
    pub author: String,
    pub source: CommentSource,
    pub body: String,
    pub streaming: bool,
    pub pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewThread {
    pub anchor: ThreadAnchor,
    pub end_line: Option<u32>,
    pub anchor_text: Option<String>,
    pub finding: Option<FindingMeta>,
    pub first_comment: Option<NewComment>,
}

impl NewThread {
    pub fn at(anchor: ThreadAnchor) -> Self {
        Self {
            anchor,
            end_line: None,
            anchor_text: None,
            finding: None,
            first_comment: None,
        }
    }
}

fn thread_id(id: i64) -> ThreadId {
    ThreadId(u32::try_from(id).unwrap_or(u32::MAX))
}

fn bump(conn: &Connection, session: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "UPDATE sessions SET revision = revision + 1 WHERE id = ?1 RETURNING revision",
        [session],
        |row| row.get(0),
    )
}

fn session_of(conn: &Connection, thread: ThreadId) -> rusqlite::Result<Option<i64>> {
    conn.query_row("SELECT session_id FROM threads WHERE id = ?1", [thread.0], |row| {
        row.get(0)
    })
    .optional()
}

const THREAD_COLUMNS: &str = "id, kind, path, side, line, end_line, anchor_text, status, severity, finding_title, \
                              finding_location, addressed_run, addressed_summary, addressed_files";

fn side_text(side: Side) -> &'static str {
    match side {
        Side::Old => "old",
        Side::New => "new",
    }
}

fn anchor_from(row: &Row) -> rusqlite::Result<ThreadAnchor> {
    let kind: String = row.get(1)?;
    let path: String = row.get(2)?;
    Ok(match kind.as_str() {
        "line" => ThreadAnchor::Line {
            path,
            side: if row.get::<_, Option<String>>(3)?.as_deref() == Some("old") {
                Side::Old
            } else {
                Side::New
            },
            line: row.get::<_, Option<u32>>(4)?.unwrap_or(0),
        },
        "file" => ThreadAnchor::File { path },
        _ => ThreadAnchor::Change,
    })
}

fn thread_from(row: &Row) -> rusqlite::Result<ReviewThread> {
    let severity = row.get::<_, Option<String>>(8)?.and_then(|text| Severity::parse(&text));
    let title: Option<String> = row.get(9)?;
    let finding = match (severity, title) {
        (Some(severity), Some(title)) => Some(FindingMeta {
            severity,
            title,
            location: row.get(10)?,
        }),
        _ => None,
    };
    let addressed = match row.get::<_, Option<i64>>(11)? {
        Some(run) => Some(Addressed {
            run: RunId(run),
            summary: row.get::<_, Option<String>>(12)?.unwrap_or_default(),
            files: row
                .get::<_, Option<String>>(13)?
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default(),
        }),
        None => None,
    };
    Ok(ReviewThread {
        id: thread_id(row.get(0)?),
        anchor: anchor_from(row)?,
        comments: Vec::new(),
        finding,
        end_line: row.get(5)?,
        anchor_text: row.get(6)?,
        status: ThreadStatus::parse(&row.get::<_, String>(7)?),
        addressed,
    })
}

fn load(conn: &Connection, session: Option<i64>, thread: Option<ThreadId>) -> rusqlite::Result<Vec<ReviewThread>> {
    let mut threads: Vec<ReviewThread> = conn
        .prepare(&format!(
            "SELECT {THREAD_COLUMNS} FROM threads WHERE (?1 IS NULL OR session_id = ?1) AND (?2 IS NULL OR id = ?2) \
             ORDER BY id"
        ))?
        .query_map((session, thread.map(|id| id.0)), thread_from)?
        .collect::<rusqlite::Result<_>>()?;
    let mut statement = conn.prepare(
        "SELECT c.thread_id, c.seq, c.author, c.source, c.body, c.streaming, c.pending
         FROM comments c JOIN threads t ON t.id = c.thread_id
         WHERE (?1 IS NULL OR t.session_id = ?1) AND (?2 IS NULL OR t.id = ?2)
         ORDER BY c.thread_id, c.seq",
    )?;
    let comments = statement.query_map((session, thread.map(|id| id.0)), |row| {
        Ok((
            thread_id(row.get(0)?),
            ThreadComment {
                id: row.get(1)?,
                author: row.get(2)?,
                source: CommentSource::parse(&row.get::<_, String>(3)?),
                body: row.get(4)?,
                is_streaming: row.get(5)?,
                pending: row.get(6)?,
            },
        ))
    })?;
    for item in comments {
        let (owner, comment) = item?;
        if let Some(thread) = threads.iter_mut().find(|thread| thread.id == owner) {
            thread.comments.push(comment);
        }
    }
    Ok(threads)
}

fn insert_comment(conn: &Connection, thread: ThreadId, new: &NewComment) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO comments (thread_id, seq, author, source, body, streaming, pending, created_at)
         VALUES (?1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM comments WHERE thread_id = ?1), ?2, ?3, ?4, ?5, ?6, ?7)",
        (
            thread.0,
            &new.author,
            new.source.as_str(),
            &new.body,
            new.streaming,
            new.pending,
            now_ms(),
        ),
    )?;
    conn.execute("UPDATE threads SET updated_at = ?2 WHERE id = ?1", (thread.0, now_ms()))?;
    Ok(())
}

impl Store {
    /// Loads the threads of a session and the revision they were read at.
    pub fn load_threads(&self, session: SessionId) -> Result<Written<Vec<ReviewThread>>> {
        let conn = self.lock();
        let revision = conn.query_row("SELECT revision FROM sessions WHERE id = ?1", [session.0], |row| {
            row.get(0)
        })?;
        Ok(Written {
            value: load(&conn, Some(session.0), None)?,
            revision,
        })
    }

    pub fn load_thread(&self, id: ThreadId) -> Result<Option<(SessionId, ReviewThread)>> {
        let conn = self.lock();
        let Some(session) = session_of(&conn, id)? else {
            return Ok(None);
        };
        Ok(load(&conn, None, Some(id))?
            .into_iter()
            .next()
            .map(|thread| (SessionId(session), thread)))
    }

    pub fn insert_thread(&self, session: SessionId, new: &NewThread) -> Result<Written<ReviewThread>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let (kind, path, side, line) = match &new.anchor {
            ThreadAnchor::File { path } => ("file", path.as_str(), None, None),
            ThreadAnchor::Line { path, side, line } => ("line", path.as_str(), Some(side_text(*side)), Some(*line)),
            ThreadAnchor::Change => ("change", "", None, None),
        };
        let finding = new.finding.as_ref();
        tx.execute(
            "INSERT INTO threads (session_id, kind, path, side, line, end_line, anchor_text, severity, finding_title,
                                  finding_location, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            (
                session.0,
                kind,
                path,
                side,
                line,
                new.end_line,
                &new.anchor_text,
                finding.map(|meta| meta.severity.as_str()),
                finding.map(|meta| &meta.title),
                finding.and_then(|meta| meta.location.as_ref()),
                now_ms(),
            ),
        )?;
        let id = thread_id(tx.last_insert_rowid());
        if let Some(comment) = &new.first_comment {
            insert_comment(&tx, id, comment)?;
        }
        let revision = bump(&tx, session.0)?;
        let thread = load(&tx, None, Some(id))?
            .into_iter()
            .next()
            .ok_or_else(|| StoreError::Invalid("The new thread was not saved.".into()))?;
        tx.commit()?;
        Ok(Written {
            value: thread,
            revision,
        })
    }

    pub fn insert_comment(&self, thread: ThreadId, new: &NewComment) -> Result<Option<Written<ReviewThread>>> {
        self.change(thread, |conn| insert_comment(conn, thread, new))
    }

    /// Adds `chunk` to the streaming comment of `thread`. None when no comment is streaming.
    pub fn append_streaming(&self, thread: ThreadId, chunk: &str, done: bool) -> Result<Option<Written<ReviewThread>>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let updated = tx.execute(
            "UPDATE comments SET body = body || ?2, streaming = ?3
             WHERE thread_id = ?1 AND streaming = 1 AND author = ?4",
            (thread.0, chunk, !done, crate::threads::CLAUDE_NAME),
        )?;
        let Some(session) = session_of(&tx, thread)?.filter(|_| updated > 0) else {
            return Ok(None);
        };
        let revision = bump(&tx, session)?;
        let value = load(&tx, None, Some(thread))?.into_iter().next();
        tx.commit()?;
        Ok(value.map(|value| Written { value, revision }))
    }

    pub fn delete_thread(&self, thread: ThreadId) -> Result<Option<i64>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let Some(session) = session_of(&tx, thread)? else {
            return Ok(None);
        };
        tx.execute("DELETE FROM threads WHERE id = ?1", [thread.0])?;
        let revision = bump(&tx, session)?;
        tx.commit()?;
        Ok(Some(revision))
    }

    pub fn set_status(&self, thread: ThreadId, status: ThreadStatus) -> Result<Option<Written<ReviewThread>>> {
        self.change(thread, |conn| {
            conn.execute(
                "UPDATE threads SET status = ?2, updated_at = ?3 WHERE id = ?1",
                (thread.0, status.as_str(), now_ms()),
            )
            .map(drop)
        })
    }

    pub fn mark_addressed(&self, thread: ThreadId, addressed: &Addressed) -> Result<Option<Written<ReviewThread>>> {
        let files = serde_json::to_string(&addressed.files).unwrap_or_else(|_| "[]".into());
        self.change(thread, |conn| {
            conn.execute(
                "UPDATE threads SET addressed_run = ?2, addressed_summary = ?3, addressed_files = ?4, updated_at = ?5
                 WHERE id = ?1",
                (thread.0, addressed.run.0, &addressed.summary, &files, now_ms()),
            )
            .map(drop)
        })
    }

    pub fn clear_addressed(&self, thread: ThreadId) -> Result<Option<Written<ReviewThread>>> {
        self.change(thread, |conn| {
            conn.execute(
                "UPDATE threads SET addressed_run = NULL, addressed_summary = NULL, addressed_files = NULL
                 WHERE id = ?1",
                [thread.0],
            )
            .map(drop)
        })
    }

    pub fn move_thread(
        &self,
        thread: ThreadId,
        line: u32,
        end_line: Option<u32>,
    ) -> Result<Option<Written<ReviewThread>>> {
        self.change(thread, |conn| {
            conn.execute(
                "UPDATE threads SET line = ?2, end_line = ?3, updated_at = ?4 WHERE id = ?1 AND kind = 'line'",
                (thread.0, line, end_line, now_ms()),
            )
            .map(drop)
        })
    }

    pub fn set_anchor_text(&self, thread: ThreadId, text: Option<&str>) -> Result<Option<Written<ReviewThread>>> {
        self.change(thread, |conn| {
            conn.execute("UPDATE threads SET anchor_text = ?2 WHERE id = ?1", (thread.0, text))
                .map(drop)
        })
    }

    /// Makes every pending comment of the session visible to runs. Returns how many changed.
    pub fn finish_pending(&self, session: SessionId) -> Result<usize> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let count = tx.execute(
            "UPDATE comments SET pending = 0 WHERE pending = 1
             AND thread_id IN (SELECT id FROM threads WHERE session_id = ?1)",
            [session.0],
        )?;
        if count > 0 {
            bump(&tx, session.0)?;
        }
        tx.commit()?;
        Ok(count)
    }

    /// A reply that was streaming when the app quit cannot continue, so it ends where it stopped.
    pub fn end_stale_streams(&self, session: SessionId) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE comments SET streaming = 0 WHERE streaming = 1
             AND thread_id IN (SELECT id FROM threads WHERE session_id = ?1)",
            [session.0],
        )?;
        Ok(())
    }

    fn change(
        &self,
        thread: ThreadId,
        apply: impl FnOnce(&Connection) -> rusqlite::Result<()>,
    ) -> Result<Option<Written<ReviewThread>>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let Some(session) = session_of(&tx, thread)? else {
            return Ok(None);
        };
        apply(&tx)?;
        let revision = bump(&tx, session)?;
        let value = load(&tx, None, Some(thread))?.into_iter().next();
        tx.commit()?;
        Ok(value.map(|value| Written { value, revision }))
    }
}
