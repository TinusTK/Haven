use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::{Serialize, de::DeserializeOwned};

use crate::{Error, Result};

/// Schema steps, applied in order. Never edit a step that has shipped:
/// add a new one. The index + 1 is the schema version.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE kv (
        key   TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL
    ) STRICT;",
];

/// Haven's local SQLite database.
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::prepare(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Read a value saved with [`Store::put`]; `None` if never saved.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM kv WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        raw.map(|json| serde_json::from_str(&json).map_err(Error::from))
            .transpose()
    }

    pub fn put<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let json = serde_json::to_string(value)?;
        self.conn.execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (key, json),
        )?;
        Ok(())
    }

    pub fn remove(&self, key: &str) -> Result<()> {
        self.conn.execute("DELETE FROM kv WHERE key = ?1", [key])?;
        Ok(())
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let known = MIGRATIONS.len() as u32;
    let found: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if found > known {
        return Err(Error::DatabaseTooNew { found, known });
    }
    for (index, step) in MIGRATIONS.iter().enumerate().skip(found as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(step)?;
        tx.pragma_update(None, "user_version", index as u32 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_database_is_at_the_latest_schema() {
        let store = Store::in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), MIGRATIONS.len() as u32);
    }

    #[test]
    fn values_round_trip_and_overwrite() {
        let store = Store::in_memory().unwrap();
        assert_eq!(store.get::<u32>("n").unwrap(), None);
        store.put("n", &1u32).unwrap();
        store.put("n", &2u32).unwrap();
        assert_eq!(store.get::<u32>("n").unwrap(), Some(2));
        store.remove("n").unwrap();
        assert_eq!(store.get::<u32>("n").unwrap(), None);
    }

    #[test]
    fn reopening_keeps_data_and_does_not_rerun_migrations() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.sqlite3");
        Store::open(&path).unwrap().put("k", &"v").unwrap();
        let again = Store::open(&path).unwrap();
        assert_eq!(again.get::<String>("k").unwrap().as_deref(), Some("v"));
    }

    #[test]
    fn refuses_a_database_from_a_newer_build() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.sqlite3");
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", 999)
            .unwrap();
        assert!(matches!(
            Store::open(&path),
            Err(Error::DatabaseTooNew { found: 999, .. })
        ));
    }
}
