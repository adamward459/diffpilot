use super::{Result, Store};
use crate::viewed::ViewedKey;
use std::collections::BTreeMap;

impl Store {
    pub fn viewed(&self, key: &ViewedKey) -> Result<BTreeMap<String, String>> {
        let conn = self.lock();
        let mut statement = conn.prepare("SELECT path, fingerprint FROM viewed WHERE repo = ?1 AND branch = ?2")?;
        let rows = statement.query_map((&key.repo, &key.branch), |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_viewed(&self, key: &ViewedKey, marks: &BTreeMap<String, String>) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM viewed WHERE repo = ?1 AND branch = ?2",
            (&key.repo, &key.branch),
        )?;
        let mut insert = tx.prepare("INSERT INTO viewed (repo, branch, path, fingerprint) VALUES (?1, ?2, ?3, ?4)")?;
        for (path, fingerprint) in marks {
            insert.execute((&key.repo, &key.branch, path, fingerprint))?;
        }
        drop(insert);
        Ok(tx.commit()?)
    }
}
