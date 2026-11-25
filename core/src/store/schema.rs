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
    "
    CREATE TABLE guides (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      session_id INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
      run_id INTEGER,
      state TEXT NOT NULL CHECK (state IN ('draft', 'done')),
      model TEXT NOT NULL DEFAULT '',
      fingerprint TEXT NOT NULL,
      created_at INTEGER NOT NULL,
      finished_at INTEGER,
      data TEXT NOT NULL
    );
    CREATE INDEX idx_guides_session ON guides (session_id, state, id);
    CREATE UNIQUE INDEX idx_guides_run ON guides (run_id) WHERE run_id IS NOT NULL;
    ",
];

/// Each entry of `MIGRATIONS` runs once, in order, and `user_version` counts the ones done.
pub(super) fn migrate(conn: &Connection) -> Result<()> {
    migrate_to(conn, MIGRATIONS.len())
}

fn migrate_to(conn: &Connection, count: usize) -> Result<()> {
    let done: usize = conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))? as usize;
    if done > MIGRATIONS.len() {
        return Err(StoreError::TooNew);
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().take(count).skip(done) {
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
    fn a_database_from_before_guides_keeps_its_rows_and_gains_the_guides_table() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_to(&conn, 2).unwrap();
        conn.execute_batch(
            "INSERT INTO sessions (repo, branch, base, created_at) VALUES ('/r', 'topic', 'main', 1);
             INSERT INTO threads (session_id, kind, path, created_at, updated_at) VALUES (1, 'file', 'a.ts', 1, 1);
             INSERT INTO runs (session_id, kind, title, state, model, queued_at, data)
               VALUES (1, 'review', 'Review', 'done', 'opus high', 1, '{}');",
        )
        .unwrap();
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, 2);
        assert!(conn.prepare("SELECT * FROM guides").is_err());

        migrate(&conn).unwrap();

        let count = |table: &str| -> i64 {
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(
            (count("sessions"), count("threads"), count("runs"), count("guides")),
            (1, 1, 1, 0)
        );
        conn.execute(
            "INSERT INTO guides (session_id, state, fingerprint, created_at, data) VALUES (1, 'draft', 'f', 1, '{}')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn a_newer_database_is_refused() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
            .unwrap();
        assert!(matches!(migrate(&conn), Err(StoreError::TooNew)));
    }
}
