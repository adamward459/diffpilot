use super::{Result, StoreError};
use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    "
    CREATE TABLE sessions (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      repo TEXT NOT NULL,
      branch TEXT NOT NULL,
      base TEXT NOT NULL,
      revision INTEGER NOT NULL DEFAULT 0,
      created_at INTEGER NOT NULL,
      UNIQUE (repo, branch, base)
    );

    CREATE TABLE threads (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      session_id INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
      kind TEXT NOT NULL CHECK (kind IN ('file', 'line', 'change')),
      path TEXT NOT NULL DEFAULT '',
      side TEXT,
      line INTEGER,
      end_line INTEGER,
      anchor_text TEXT,
      status TEXT NOT NULL DEFAULT 'open',
      severity TEXT,
      finding_title TEXT,
      finding_location TEXT,
      addressed_run INTEGER,
      addressed_summary TEXT,
      addressed_files TEXT,
      created_at INTEGER NOT NULL,
      updated_at INTEGER NOT NULL
    );
    CREATE INDEX idx_threads_session ON threads (session_id);

    CREATE TABLE comments (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      thread_id INTEGER NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
      seq INTEGER NOT NULL,
      author TEXT NOT NULL,
      source TEXT NOT NULL DEFAULT 'user',
      body TEXT NOT NULL,
      streaming INTEGER NOT NULL DEFAULT 0,
      pending INTEGER NOT NULL DEFAULT 0,
      created_at INTEGER NOT NULL,
      UNIQUE (thread_id, seq)
    );
    ",
    "
    CREATE TABLE runs (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      session_id INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
      kind TEXT NOT NULL,
      title TEXT NOT NULL,
      state TEXT NOT NULL,
      model TEXT NOT NULL,
      queued_at INTEGER NOT NULL,
      started_at INTEGER,
      finished_at INTEGER,
      data TEXT NOT NULL
    );
    CREATE INDEX idx_runs_session ON runs (session_id);
    ",
];

/// Each entry of `MIGRATIONS` runs once, in order, and `user_version` counts the ones done.
pub(super) fn migrate(conn: &Connection) -> Result<()> {
    let done: usize = conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))? as usize;
    if done > MIGRATIONS.len() {
        return Err(StoreError::TooNew);
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(done) {
        conn.execute_batch(&format!("BEGIN;\n{sql}\nPRAGMA user_version = {};\nCOMMIT;", index + 1))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrating_twice_changes_nothing() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
    }

    #[test]
    fn a_newer_database_is_refused() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
            .unwrap();
        assert!(matches!(migrate(&conn), Err(StoreError::TooNew)));
    }
}
