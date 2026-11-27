use super::{Result, SessionId, Store, now_ms};
use crate::review::{Effort, ModelName, ReviewModel};
use crate::run::{Run, RunKind, RunState, RunSummary};
use crate::store::RunId;

fn kind_of(text: &str) -> RunKind {
    serde_json::from_value(serde_json::Value::String(text.into())).unwrap_or(RunKind::MakeChanges)
}

fn state_of(text: &str) -> RunState {
    serde_json::from_value(serde_json::Value::String(text.into())).unwrap_or(RunState::Failed)
}

fn name(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(String::from))
        .unwrap_or_default()
}

fn sql_limit(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

fn summary_of(row: &rusqlite::Row) -> rusqlite::Result<RunSummary> {
    let model: String = row.get(4)?;
    let (model_name, effort) = model.split_once(' ').unwrap_or(("opus", "high"));
    Ok(RunSummary {
        id: RunId(row.get(0)?),
        kind: kind_of(&row.get::<_, String>(1)?),
        title: row.get(2)?,
        state: state_of(&row.get::<_, String>(3)?),
        model: ReviewModel::new(
            serde_json::from_value(serde_json::Value::String(model_name.into())).unwrap_or(ModelName::Opus),
            serde_json::from_value(serde_json::Value::String(effort.into())).unwrap_or(Effort::High),
        ),
        started_at_ms: row.get(5)?,
        finished_at_ms: row.get(6)?,
    })
}

impl Store {
    /// Reserves the id of a run. Call `save_run` right after, so the row has its data.
    pub fn insert_run(&self, session: SessionId, kind: RunKind, title: &str) -> Result<RunId> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO runs (session_id, kind, title, state, model, queued_at, data)
             VALUES (?1, ?2, ?3, 'queued', '', ?4, '{}')",
            (session.0, name(&kind), title, now_ms()),
        )?;
        Ok(RunId(conn.last_insert_rowid()))
    }

    pub fn save_run(&self, run: &Run) -> Result<()> {
        let data = serde_json::to_string(run).map_err(|error| super::StoreError::Invalid(error.to_string()))?;
        self.lock().execute(
            "UPDATE runs SET title = ?2, state = ?3, model = ?4, started_at = ?5, finished_at = ?6, data = ?7
             WHERE id = ?1",
            (
                run.id.0,
                &run.title,
                name(&run.state),
                format!("{} {}", name(&run.model.name), name(&run.model.effort)),
                run.started_at_ms,
                run.finished_at_ms,
                data,
            ),
        )?;
        Ok(())
    }

    pub fn load_run(&self, id: RunId) -> Result<Option<Run>> {
        let conn = self.lock();
        let mut statement = conn.prepare("SELECT data FROM runs WHERE id = ?1")?;
        let mut rows = statement.query_map([id.0], |row| row.get::<_, String>(0))?;
        Ok(rows
            .next()
            .transpose()?
            .and_then(|data| serde_json::from_str(&data).ok()))
    }

    /// Newest first, without the logs.
    pub fn run_summaries(&self, session: SessionId, limit: usize) -> Result<Vec<RunSummary>> {
        let conn = self.lock();
        let mut statement = conn.prepare(
            "SELECT id, kind, title, state, model, started_at, finished_at FROM runs
             WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map((session.0, sql_limit(limit)), summary_of)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Runs of every kind and every comparison of one branch, newest first, without the logs.
    pub fn branch_run_summaries(&self, repo: &str, branch: &str, limit: usize) -> Result<Vec<RunSummary>> {
        let conn = self.lock();
        let mut statement = conn.prepare(
            "SELECT runs.id, runs.kind, runs.title, runs.state, runs.model, runs.started_at, runs.finished_at
             FROM runs JOIN sessions ON sessions.id = runs.session_id
             WHERE sessions.repo = ?1 AND sessions.branch = ?2 ORDER BY runs.id DESC LIMIT ?3",
        )?;
        let rows = statement.query_map((repo, branch, sql_limit(limit)), summary_of)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Runs left queued, running or paused by an app that quit. They cannot continue: a paused run counts as stopped.
    pub fn fail_stale_runs(&self) -> Result<usize> {
        let conn = self.lock();
        let now = now_ms();
        conn.execute(
            "UPDATE runs SET state = 'stopped', finished_at = ?1,
               data = json_set(data, '$.state', 'stopped', '$.finished_at_ms', ?1)
             WHERE state = 'paused'",
            [now],
        )?;
        let failed = conn.execute(
            "UPDATE runs SET state = 'failed', finished_at = ?1,
               data = json_set(data, '$.state', 'failed', '$.finished_at_ms', ?1, '$.error', 'diffpilot quit during this run.')
             WHERE state IN ('queued', 'running')",
            [now],
        )?;
        conn.execute(
            "DELETE FROM guides WHERE state = 'draft'
             AND run_id NOT IN (SELECT id FROM runs WHERE state IN ('queued', 'running'))",
            [],
        )?;
        Ok(failed)
    }
}
