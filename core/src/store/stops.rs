use super::{GuideId, Result, Store};
use std::collections::BTreeSet;

impl Store {
    pub fn reviewed_stops(&self, guide: GuideId) -> Result<BTreeSet<(usize, usize)>> {
        let conn = self.lock();
        let mut statement = conn.prepare("SELECT chapter, note FROM reviewed_stops WHERE guide_id = ?1")?;
        let rows = statement.query_map([guide.0], |row| {
            Ok((row.get::<_, i64>(0)? as usize, row.get::<_, i64>(1)? as usize))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_stop_reviewed(&self, guide: GuideId, chapter: usize, note: usize, reviewed: bool) -> Result<()> {
        let conn = self.lock();
        if reviewed {
            conn.execute(
                "INSERT OR IGNORE INTO reviewed_stops (guide_id, chapter, note) VALUES (?1, ?2, ?3)",
                (guide.0, chapter as i64, note as i64),
            )?;
        } else {
            conn.execute(
                "DELETE FROM reviewed_stops WHERE guide_id = ?1 AND chapter = ?2 AND note = ?3",
                (guide.0, chapter as i64, note as i64),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::Guide;
    use crate::review::ReviewModel;
    use crate::store::{RunId, SessionKey};

    fn guide_in(store: &Store, run: i64) -> GuideId {
        let session = store.session(&SessionKey::new("/r", "topic", "main")).unwrap();
        store
            .save_draft(session, RunId(run), ReviewModel::STANDARD, &Guide::default())
            .unwrap()
    }

    #[test]
    fn marking_a_stop_reviewed_twice_keeps_one_row_and_unmarking_removes_it() {
        let store = Store::open_in_memory().unwrap();
        let guide = guide_in(&store, 1);
        assert!(store.reviewed_stops(guide).unwrap().is_empty());
        store.set_stop_reviewed(guide, 0, 1, true).unwrap();
        store.set_stop_reviewed(guide, 0, 1, true).unwrap();
        store.set_stop_reviewed(guide, 2, 0, true).unwrap();
        assert_eq!(store.reviewed_stops(guide).unwrap(), [(0, 1), (2, 0)].into());
        store.set_stop_reviewed(guide, 0, 1, false).unwrap();
        store.set_stop_reviewed(guide, 0, 1, false).unwrap();
        assert_eq!(store.reviewed_stops(guide).unwrap(), [(2, 0)].into());
    }

    #[test]
    fn each_guide_has_its_own_progress_and_a_deleted_guide_takes_it_along() {
        let store = Store::open_in_memory().unwrap();
        let (first, second) = (guide_in(&store, 1), guide_in(&store, 2));
        store.set_stop_reviewed(first, 0, 0, true).unwrap();
        assert!(store.reviewed_stops(second).unwrap().is_empty());
        store.delete_guide(first).unwrap();
        assert!(store.reviewed_stops(first).unwrap().is_empty());
    }

    #[test]
    fn progress_survives_reopening_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("review.sqlite3");
        let guide = {
            let store = Store::open(&path).unwrap();
            let guide = guide_in(&store, 1);
            store.set_stop_reviewed(guide, 1, 3, true).unwrap();
            guide
        };
        assert_eq!(
            Store::open(&path).unwrap().reviewed_stops(guide).unwrap(),
            [(1, 3)].into()
        );
    }
}
