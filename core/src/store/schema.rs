use super::Result;
use rusqlite::Connection;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS sessions (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      repo TEXT NOT NULL,
      branch TEXT NOT NULL,
      base TEXT NOT NULL,
      revision INTEGER NOT NULL DEFAULT 0,
      created_at INTEGER NOT NULL,
      UNIQUE (repo, branch, base)
    );

    CREATE TABLE IF NOT EXISTS threads (
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
    CREATE INDEX IF NOT EXISTS idx_threads_session ON threads (session_id);

    CREATE TABLE IF NOT EXISTS comments (
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
    );    CREATE TABLE IF NOT EXISTS runs (
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
    CREATE INDEX IF NOT EXISTS idx_runs_session ON runs (session_id);    CREATE TABLE IF NOT EXISTS guides (
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
    CREATE INDEX IF NOT EXISTS idx_guides_session ON guides (session_id, state, id);
    CREATE UNIQUE INDEX IF NOT EXISTS idx_guides_run ON guides (run_id) WHERE run_id IS NOT NULL;    CREATE TABLE IF NOT EXISTS viewed (
      repo TEXT NOT NULL,
      branch TEXT NOT NULL,
      path TEXT NOT NULL,
      fingerprint TEXT NOT NULL,
      PRIMARY KEY (repo, branch, path)
    ) WITHOUT ROWID;
";

pub(super) fn create(conn: &Connection) -> Result<()> {
    Ok(conn.execute_batch(SCHEMA)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creating_twice_keeps_the_rows() {
        let conn = Connection::open_in_memory().unwrap();
        create(&conn).unwrap();
        conn.execute_batch("INSERT INTO sessions (repo, branch, base, created_at) VALUES ('/r', 'topic', 'main', 1);")
            .unwrap();
        create(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
