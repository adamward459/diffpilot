use super::{GuideId, Result, RunId, SessionId, Store, StoreError, now_ms};
use crate::guide::Guide;
use crate::review::{Effort, ModelName, ReviewModel};
use rusqlite::{OptionalExtension, Row};

/// A guide as saved: the content, and the run that wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredGuide {
    pub id: GuideId,
    pub session: SessionId,
    pub run: Option<RunId>,
    pub model: Option<ReviewModel>,
    pub created_at_ms: i64,
    /// None while a run is still writing it.
    pub finished_at_ms: Option<i64>,
    pub guide: Guide,
}

/// One row of the Past Guides menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideSummary {
    pub id: GuideId,
    pub run: Option<RunId>,
    pub title: String,
    pub chapters: u32,
    pub model: Option<ReviewModel>,
    pub created_at_ms: i64,
    pub finished_at_ms: Option<i64>,
}

fn model_text(model: ReviewModel) -> String {
    format!("{} {}", model.name.as_str(), model.effort.as_str())
}

fn model_of(text: &str) -> Option<ReviewModel> {
    let (name, effort) = text.split_once(' ')?;
    let name = ModelName::ALL.into_iter().find(|known| known.as_str() == name)?;
    let effort = Effort::ALL.into_iter().find(|known| known.as_str() == effort)?;
    Some(ReviewModel::new(name, effort))
}

const COLUMNS: &str = "id, session_id, run_id, model, created_at, finished_at, data";

fn stored(row: &Row) -> rusqlite::Result<StoredGuide> {
    let data: String = row.get(6)?;
    Ok(StoredGuide {
        id: GuideId(row.get(0)?),
        session: SessionId(row.get(1)?),
        run: row.get::<_, Option<i64>>(2)?.map(RunId),
        model: model_of(&row.get::<_, String>(3)?),
        created_at_ms: row.get(4)?,
        finished_at_ms: row.get(5)?,
        guide: serde_json::from_str(&data).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error))
        })?,
    })
}

fn encode(guide: &Guide) -> Result<String> {
    serde_json::to_string(guide).map_err(|error| StoreError::Invalid(error.to_string()))
}

impl Store {
    /// Saves the guide a run is still writing. The run has one draft, and each call replaces it.
    pub fn save_draft(&self, session: SessionId, run: RunId, model: ReviewModel, guide: &Guide) -> Result<GuideId> {
        let data = encode(guide)?;
        let conn = self.lock();
        conn.execute(
            "INSERT INTO guides (session_id, run_id, state, model, fingerprint, created_at, data)
             VALUES (?1, ?2, 'draft', ?3, ?4, ?5, ?6)
             ON CONFLICT (run_id) WHERE run_id IS NOT NULL DO UPDATE SET data = excluded.data
             WHERE guides.state = 'draft'",
            (session.0, run.0, model_text(model), &guide.fingerprint, now_ms(), data),
        )?;
        Ok(GuideId(conn.query_row(
            "SELECT id FROM guides WHERE run_id = ?1",
            [run.0],
            |row| row.get(0),
        )?))
    }

    pub fn load_draft(&self, run: RunId) -> Result<Option<StoredGuide>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM guides WHERE run_id = ?1 AND state = 'draft'"),
                [run.0],
                stored,
            )
            .optional()?)
    }

    /// Makes the run's draft the current guide of its review. The guides before it become Past Guides.
    pub fn complete_draft(&self, run: RunId, guide: &Guide) -> Result<Option<GuideId>> {
        let data = encode(guide)?;
        let conn = self.lock();
        Ok(conn
            .query_row(
                "UPDATE guides SET state = 'done', finished_at = ?2, data = ?3, fingerprint = ?4
                 WHERE run_id = ?1 AND state = 'draft' RETURNING id",
                (run.0, now_ms(), data, &guide.fingerprint),
                |row| row.get(0),
            )
            .optional()?
            .map(GuideId))
    }

    /// The finished guide this run wrote, if it got that far.
    pub fn guide_of_run(&self, run: RunId) -> Result<Option<GuideId>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT id FROM guides WHERE run_id = ?1 AND state = 'done'",
                [run.0],
                |row| row.get(0),
            )
            .optional()?
            .map(GuideId))
    }

    pub fn discard_draft(&self, run: RunId) -> Result<()> {
        self.lock()
            .execute("DELETE FROM guides WHERE run_id = ?1 AND state = 'draft'", [run.0])?;
        Ok(())
    }

    /// The newest finished guide of the review.
    pub fn current_guide(&self, session: SessionId) -> Result<Option<StoredGuide>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM guides WHERE session_id = ?1 AND state = 'done' ORDER BY id DESC LIMIT 1"
                ),
                [session.0],
                stored,
            )
            .optional()?)
    }

    pub fn load_guide(&self, id: GuideId) -> Result<Option<StoredGuide>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM guides WHERE id = ?1 AND state = 'done'"),
                [id.0],
                stored,
            )
            .optional()?)
    }

    /// Finished guides of the review, newest first. The first is the current one; the rest are Past Guides.
    pub fn guides(&self, session: SessionId, limit: usize) -> Result<Vec<GuideSummary>> {
        let conn = self.lock();
        let mut statement = conn.prepare(
            "SELECT id, run_id, json_extract(data, '$.overview.title'), json_array_length(data, '$.chapters'),
                    model, created_at, finished_at
             FROM guides WHERE session_id = ?1 AND state = 'done' ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map((session.0, i64::try_from(limit).unwrap_or(i64::MAX)), |row| {
            Ok(GuideSummary {
                id: GuideId(row.get(0)?),
                run: row.get::<_, Option<i64>>(1)?.map(RunId),
                title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                chapters: row.get(3)?,
                model: model_of(&row.get::<_, String>(4)?),
                created_at_ms: row.get(5)?,
                finished_at_ms: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn delete_guide(&self, id: GuideId) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM guides WHERE id = ?1", [id.0])? > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::{Attention, Chapter, ChapterFile, Diagram, DiagramKind, FlowStep, Impact, Note, Overview, Span};
    use crate::store::SessionKey;
    use crate::threads::Side;

    pub fn sample() -> Guide {
        Guide {
            overview: Overview {
                title: "Undo keeps a timer".into(),
                summary: "A sent message waits five seconds.".into(),
                before: Some("Sent at once.".into()),
                after: None,
            },
            diagrams: vec![Diagram {
                title: Some("Flow".into()),
                kind: DiagramKind::Flowchart,
                source: "flowchart LR\n  A --> B".into(),
                flow: vec![FlowStep {
                    node: "A".into(),
                    text: "Start.".into(),
                }],
            }],
            chapters: vec![Chapter {
                title: "The timer".into(),
                summary: "Why it exists.".into(),
                phase: Some("Core".into()),
                attention: Attention::High,
                points: vec!["One timer per message.".into()],
                focus: vec![],
                files: vec![ChapterFile {
                    path: "a.ts".into(),
                    spans: vec![Span {
                        side: Side::New,
                        line: 2,
                        end_line: Some(3),
                        anchor_text: Some("x\ny".into()),
                    }],
                }],
                notes: vec![Note {
                    path: "a.ts".into(),
                    span: None,
                    text: "Check the unit.".into(),
                    critical: true,
                }],
                diagrams: vec![],
                impact: vec![Impact {
                    site: "send()".into(),
                    path: "b.ts".into(),
                    line: Some(4),
                    text: "Now waits.".into(),
                }],
            }],
            fingerprint: "abc".into(),
            file_fingerprints: [("a.ts".to_string(), "f1".to_string())].into(),
            head: Some("deadbeef".into()),
        }
    }

    fn setup() -> (Store, SessionId) {
        let store = Store::open_in_memory().unwrap();
        let session = store.session(&SessionKey::new("/r", "topic", "main")).unwrap();
        (store, session)
    }

    const MODEL: ReviewModel = ReviewModel::STANDARD;

    #[test]
    fn a_draft_is_replaced_by_each_save_and_hidden_until_it_is_completed() {
        let (store, session) = setup();
        let mut guide = sample();
        let id = store.save_draft(session, RunId(1), MODEL, &guide).unwrap();
        guide.overview.title = "Changed".into();
        assert_eq!(store.save_draft(session, RunId(1), MODEL, &guide).unwrap(), id);
        assert_eq!(
            store.load_draft(RunId(1)).unwrap().unwrap().guide.overview.title,
            "Changed"
        );
        assert_eq!(store.current_guide(session).unwrap(), None);
        assert!(store.guides(session, 10).unwrap().is_empty());

        assert_eq!(store.complete_draft(RunId(1), &guide).unwrap(), Some(id));
        assert_eq!(store.complete_draft(RunId(1), &guide).unwrap(), None);
        assert_eq!(store.load_draft(RunId(1)).unwrap(), None);
        let current = store.current_guide(session).unwrap().unwrap();
        assert_eq!(
            (current.id, current.run, current.model),
            (id, Some(RunId(1)), Some(MODEL))
        );
        assert!(current.finished_at_ms.is_some());
        assert_eq!(current.guide, guide);
    }

    #[test]
    fn a_guide_round_trips_through_a_file_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("review.sqlite3");
        let guide = sample();
        let session = {
            let store = Store::open(&path).unwrap();
            let session = store.session(&SessionKey::new("/r", "topic", "main")).unwrap();
            store.save_draft(session, RunId(7), MODEL, &guide).unwrap();
            store.complete_draft(RunId(7), &guide).unwrap();
            session
        };
        let store = Store::open(&path).unwrap();
        assert_eq!(store.current_guide(session).unwrap().unwrap().guide, guide);
    }

    #[test]
    fn a_new_guide_becomes_current_and_the_old_one_a_past_guide() {
        let (store, session) = setup();
        let mut first = sample();
        first.overview.title = "First".into();
        let mut second = sample();
        second.overview.title = "Second".into();
        second.chapters.push(second.chapters[0].clone());
        for (run, guide) in [(1, &first), (2, &second)] {
            store.save_draft(session, RunId(run), MODEL, guide).unwrap();
            store.complete_draft(RunId(run), guide).unwrap();
        }
        assert_eq!(
            store.current_guide(session).unwrap().unwrap().guide.overview.title,
            "Second"
        );
        let listed = store.guides(session, 10).unwrap();
        let rows: Vec<_> = listed
            .iter()
            .map(|row| (row.title.as_str(), row.chapters, row.run))
            .collect();
        assert_eq!(rows, [("Second", 2, Some(RunId(2))), ("First", 1, Some(RunId(1)))]);
        let past = store.load_guide(listed[1].id).unwrap().unwrap();
        assert_eq!(past.guide.overview.title, "First");
        assert!(store.delete_guide(listed[1].id).unwrap());
        assert_eq!(store.guides(session, 10).unwrap().len(), 1);
    }

    #[test]
    fn each_review_has_its_own_guide() {
        let (store, session) = setup();
        let other = store.session(&SessionKey::new("/r", "topic", "develop")).unwrap();
        store.save_draft(session, RunId(1), MODEL, &sample()).unwrap();
        store.complete_draft(RunId(1), &sample()).unwrap();
        assert!(store.current_guide(session).unwrap().is_some());
        assert_eq!(store.current_guide(other).unwrap(), None);
    }

    #[test]
    fn a_discarded_draft_and_the_drafts_of_dead_runs_leave_no_guide() {
        let (store, session) = setup();
        store.save_draft(session, RunId(1), MODEL, &sample()).unwrap();
        store.discard_draft(RunId(1)).unwrap();
        assert_eq!(store.load_draft(RunId(1)).unwrap(), None);

        store.save_draft(session, RunId(2), MODEL, &sample()).unwrap();
        let run = store
            .insert_run(session, crate::run::RunKind::Guide, "Writing a guide")
            .unwrap();
        store.save_draft(session, run, MODEL, &sample()).unwrap();
        store.fail_stale_runs().unwrap();
        assert_eq!(
            store.load_draft(RunId(2)).unwrap(),
            None,
            "no run row, so no run is writing it"
        );
        assert_eq!(
            store.load_draft(run).unwrap(),
            None,
            "the run was queued when the app quit"
        );
    }
}
