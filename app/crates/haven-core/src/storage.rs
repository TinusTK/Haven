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
    "CREATE TABLE addons (
        url      TEXT PRIMARY KEY NOT NULL,
        manifest TEXT NOT NULL,
        position INTEGER NOT NULL
    ) STRICT;",
    "CREATE TABLE progress (
        kind       TEXT NOT NULL,
        title_id   TEXT NOT NULL,
        video_id   TEXT NOT NULL,
        title      TEXT NOT NULL,
        detail     TEXT NOT NULL,
        poster     TEXT,
        position   REAL NOT NULL,
        duration   REAL NOT NULL,
        finished   INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (kind, title_id, video_id)
    ) STRICT;
    CREATE INDEX progress_recent ON progress (updated_at DESC);",
    "CREATE TABLE watchlist (
        kind     TEXT NOT NULL,
        title_id TEXT NOT NULL,
        title    TEXT NOT NULL,
        year     TEXT NOT NULL,
        poster   TEXT,
        added_at INTEGER NOT NULL,
        PRIMARY KEY (kind, title_id)
    ) STRICT;",
    "CREATE TABLE tv_sources (
        url      TEXT PRIMARY KEY NOT NULL,
        name     TEXT NOT NULL,
        playlist TEXT NOT NULL,
        position INTEGER NOT NULL
    ) STRICT;",
    "CREATE TABLE servers (
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        kind    TEXT NOT NULL,
        name    TEXT NOT NULL,
        address TEXT NOT NULL,
        user_id TEXT NOT NULL,
        token   TEXT NOT NULL,
        UNIQUE (kind, address, user_id)
    ) STRICT;",
];

/// An installed add-on as kept in the database: where it lives and the
/// manifest it served when it was installed, as raw JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAddon {
    pub url: String,
    pub manifest: String,
}

/// How far one film or episode has been watched.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    /// Content type: "movie", "series"…
    pub kind: String,
    /// The title's id (for an episode, its series).
    pub title_id: String,
    /// The film's or episode's own id; what streams are asked for.
    pub video_id: String,
    /// The title's name, for showing without asking an add-on.
    pub title: String,
    /// For an episode, which one ("S1 E2 · Name"); empty for a film.
    pub detail: String,
    /// Address of the title's poster, if known.
    pub poster: Option<String>,
    /// Seconds from the start.
    pub position: f64,
    /// Running time in seconds.
    pub duration: f64,
    /// Watched to the end (or near enough).
    pub finished: bool,
    /// When this was saved, in milliseconds since 1970.
    pub updated_at: i64,
}

/// A media server the user signed in to, as kept in the database.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredServer {
    /// Its number here; 0 before it is saved.
    pub id: i64,
    /// "jellyfin", "emby" or "plex".
    pub kind: String,
    pub name: String,
    pub address: String,
    pub user_id: String,
    /// What the server gave in exchange for signing in. The password is
    /// never kept.
    pub token: String,
}

/// Never print the token.
impl std::fmt::Debug for StoredServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredServer")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

/// A source of television channels and radio stations: a playlist's
/// address and the playlist as it was last fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TvSource {
    pub url: String,
    pub name: String,
    /// The playlist's text, kept so channels show before the network answers.
    pub playlist: String,
}

/// A title saved to watch later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// Content type: "movie", "series"…
    pub kind: String,
    pub title_id: String,
    pub title: String,
    /// Year or years, as the add-on gave them; may be empty.
    pub year: String,
    /// Address of the poster, if known.
    pub poster: Option<String>,
    /// When it was saved, in milliseconds since 1970.
    pub added_at: i64,
}

const PROGRESS_COLUMNS: &str =
    "kind, title_id, video_id, title, detail, poster, position, duration, finished, updated_at";

fn progress_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<Progress> {
    Ok(Progress {
        kind: row.get(0)?,
        title_id: row.get(1)?,
        video_id: row.get(2)?,
        title: row.get(3)?,
        detail: row.get(4)?,
        poster: row.get(5)?,
        position: row.get(6)?,
        duration: row.get(7)?,
        finished: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

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

    /// Installed add-ons, in the order they were installed.
    pub fn addons(&self) -> Result<Vec<StoredAddon>> {
        let mut query = self
            .conn
            .prepare("SELECT url, manifest FROM addons ORDER BY position, url")?;
        let rows = query.query_map([], |row| {
            Ok(StoredAddon {
                url: row.get(0)?,
                manifest: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Adds an add-on at the end, or refreshes the manifest of one that is
    /// already installed (it keeps its place).
    pub fn save_addon(&self, url: &str, manifest: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO addons (url, manifest, position)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1 FROM addons))
             ON CONFLICT(url) DO UPDATE SET manifest = excluded.manifest",
            (url, manifest),
        )?;
        Ok(())
    }

    pub fn remove_addon(&self, url: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM addons WHERE url = ?1", [url])?;
        Ok(())
    }

    /// Saves how far a film or episode was watched, replacing what was
    /// saved for it before. A poster already on record is kept when the
    /// new entry has none.
    pub fn save_progress(&self, progress: &Progress) -> Result<()> {
        self.conn.execute(
            "INSERT INTO progress (kind, title_id, video_id, title, detail, poster,
                                   position, duration, finished, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(kind, title_id, video_id) DO UPDATE SET
                title = excluded.title,
                detail = excluded.detail,
                poster = COALESCE(excluded.poster, poster),
                position = excluded.position,
                duration = excluded.duration,
                finished = excluded.finished,
                updated_at = excluded.updated_at",
            rusqlite::params![
                progress.kind,
                progress.title_id,
                progress.video_id,
                progress.title,
                progress.detail,
                progress.poster,
                progress.position,
                progress.duration,
                progress.finished,
                progress.updated_at,
            ],
        )?;
        Ok(())
    }

    /// What was saved for one film or episode, if anything.
    pub fn progress(&self, kind: &str, title_id: &str, video_id: &str) -> Result<Option<Progress>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {PROGRESS_COLUMNS} FROM progress
                     WHERE kind = ?1 AND title_id = ?2 AND video_id = ?3"
                ),
                [kind, title_id, video_id],
                progress_from,
            )
            .optional()?)
    }

    /// Titles to carry on with, most recent first: for each title the
    /// film or episode watched last, unless that one was finished.
    pub fn continue_watching(&self, limit: u32) -> Result<Vec<Progress>> {
        let mut query = self.conn.prepare(&format!(
            "SELECT {PROGRESS_COLUMNS} FROM progress AS p
             WHERE finished = 0
               AND NOT EXISTS (
                   SELECT 1 FROM progress AS q
                   WHERE q.kind = p.kind AND q.title_id = p.title_id
                     AND (q.updated_at > p.updated_at
                          OR (q.updated_at = p.updated_at AND q.video_id > p.video_id)))
             ORDER BY updated_at DESC, title_id
             LIMIT ?1"
        ))?;
        let rows = query.query_map([limit], progress_from)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The media servers, in the order they were added.
    pub fn servers(&self) -> Result<Vec<StoredServer>> {
        let mut query = self
            .conn
            .prepare("SELECT id, kind, name, address, user_id, token FROM servers ORDER BY id")?;
        let rows = query.query_map([], |row| {
            Ok(StoredServer {
                id: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                address: row.get(3)?,
                user_id: row.get(4)?,
                token: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Adds a server, or refreshes the name and token of one already
    /// there (same kind, address and user). Returns its number.
    pub fn save_server(&self, server: &StoredServer) -> Result<i64> {
        Ok(self.conn.query_row(
            "INSERT INTO servers (kind, name, address, user_id, token)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(kind, address, user_id) DO UPDATE SET
                name = excluded.name, token = excluded.token
             RETURNING id",
            (&server.kind, &server.name, &server.address, &server.user_id, &server.token),
            |row| row.get(0),
        )?)
    }

    pub fn remove_server(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM servers WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Television and radio sources, in the order they were added.
    pub fn tv_sources(&self) -> Result<Vec<TvSource>> {
        let mut query = self
            .conn
            .prepare("SELECT url, name, playlist FROM tv_sources ORDER BY position, url")?;
        let rows = query.query_map([], |row| {
            Ok(TvSource { url: row.get(0)?, name: row.get(1)?, playlist: row.get(2)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Adds a source at the end, or refreshes the playlist of one that is
    /// already there (it keeps its place and name).
    pub fn save_tv_source(&self, source: &TvSource) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tv_sources (url, name, playlist, position)
             VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), 0) + 1 FROM tv_sources))
             ON CONFLICT(url) DO UPDATE SET playlist = excluded.playlist",
            (&source.url, &source.name, &source.playlist),
        )?;
        Ok(())
    }

    pub fn remove_tv_source(&self, url: &str) -> Result<()> {
        self.conn.execute("DELETE FROM tv_sources WHERE url = ?1", [url])?;
        Ok(())
    }

    /// The watchlist, newest first.
    pub fn watchlist(&self) -> Result<Vec<Saved>> {
        let mut query = self.conn.prepare(
            "SELECT kind, title_id, title, year, poster, added_at FROM watchlist
             ORDER BY added_at DESC, title",
        )?;
        let rows = query.query_map([], |row| {
            Ok(Saved {
                kind: row.get(0)?,
                title_id: row.get(1)?,
                title: row.get(2)?,
                year: row.get(3)?,
                poster: row.get(4)?,
                added_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Saves a title to watch later. Saving it again keeps its place in
    /// the list and refreshes its name, year and poster.
    pub fn add_to_watchlist(&self, saved: &Saved) -> Result<()> {
        self.conn.execute(
            "INSERT INTO watchlist (kind, title_id, title, year, poster, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(kind, title_id) DO UPDATE SET
                title = excluded.title,
                year = excluded.year,
                poster = COALESCE(excluded.poster, poster)",
            rusqlite::params![
                saved.kind,
                saved.title_id,
                saved.title,
                saved.year,
                saved.poster,
                saved.added_at
            ],
        )?;
        Ok(())
    }

    pub fn remove_from_watchlist(&self, kind: &str, title_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM watchlist WHERE kind = ?1 AND title_id = ?2",
            [kind, title_id],
        )?;
        Ok(())
    }

    pub fn in_watchlist(&self, kind: &str, title_id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM watchlist WHERE kind = ?1 AND title_id = ?2)",
            [kind, title_id],
            |row| row.get(0),
        )?)
    }

    /// Forgets everything watched of one title (all its episodes).
    pub fn remove_progress(&self, kind: &str, title_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM progress WHERE kind = ?1 AND title_id = ?2",
            [kind, title_id],
        )?;
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
    fn addons_keep_install_order_and_update_in_place() {
        let store = Store::in_memory().unwrap();
        store.save_addon("https://a/manifest.json", "{\"v\":1}").unwrap();
        store.save_addon("https://b/manifest.json", "{}").unwrap();
        store.save_addon("https://a/manifest.json", "{\"v\":2}").unwrap();
        let urls: Vec<_> = store.addons().unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].url, "https://a/manifest.json");
        assert_eq!(urls[0].manifest, "{\"v\":2}");
        store.remove_addon("https://a/manifest.json").unwrap();
        assert_eq!(store.addons().unwrap()[0].url, "https://b/manifest.json");
    }

    fn watched(title_id: &str, video_id: &str, position: f64, finished: bool, at: i64) -> Progress {
        Progress {
            kind: "series".into(),
            title_id: title_id.into(),
            video_id: video_id.into(),
            title: format!("Show {title_id}"),
            detail: video_id.into(),
            poster: None,
            position,
            duration: 3000.0,
            finished,
            updated_at: at,
        }
    }

    #[test]
    fn progress_round_trips_and_is_replaced() {
        let store = Store::in_memory().unwrap();
        assert_eq!(store.progress("series", "a", "a:1:1").unwrap(), None);
        let mut first = watched("a", "a:1:1", 100.0, false, 1);
        first.poster = Some("https://example.org/p.jpg".into());
        store.save_progress(&first).unwrap();
        assert_eq!(store.progress("series", "a", "a:1:1").unwrap(), Some(first));

        // Saved again without a poster: the place moves, the poster stays.
        store.save_progress(&watched("a", "a:1:1", 250.0, false, 2)).unwrap();
        let again = store.progress("series", "a", "a:1:1").unwrap().unwrap();
        assert_eq!(again.position, 250.0);
        assert_eq!(again.updated_at, 2);
        assert_eq!(again.poster.as_deref(), Some("https://example.org/p.jpg"));
    }

    #[test]
    fn continue_watching_shows_each_title_once_by_its_latest_episode() {
        let store = Store::in_memory().unwrap();
        store.save_progress(&watched("a", "a:1:1", 2900.0, true, 10)).unwrap();
        store.save_progress(&watched("a", "a:1:2", 400.0, false, 20)).unwrap();
        store.save_progress(&watched("b", "b:1:1", 300.0, false, 30)).unwrap();
        // The latest episode of c was finished: nothing to carry on with.
        store.save_progress(&watched("c", "c:1:1", 300.0, false, 5)).unwrap();
        store.save_progress(&watched("c", "c:1:2", 2900.0, true, 40)).unwrap();

        let list: Vec<String> =
            store.continue_watching(10).unwrap().into_iter().map(|p| p.video_id).collect();
        assert_eq!(list, ["b:1:1", "a:1:2"]);
        assert_eq!(store.continue_watching(1).unwrap().len(), 1);

        store.remove_progress("series", "b").unwrap();
        let list: Vec<String> =
            store.continue_watching(10).unwrap().into_iter().map(|p| p.video_id).collect();
        assert_eq!(list, ["a:1:2"]);
    }

    #[test]
    fn servers_get_numbers_and_signing_in_again_keeps_the_number() {
        let store = Store::in_memory().unwrap();
        let server = |address: &str, token: &str| StoredServer {
            id: 0,
            kind: "jellyfin".into(),
            name: "Home".into(),
            address: address.into(),
            user_id: "u".into(),
            token: token.into(),
        };
        let first = store.save_server(&server("http://a", "one")).unwrap();
        let second = store.save_server(&server("http://b", "two")).unwrap();
        assert_ne!(first, second);
        assert_eq!(store.save_server(&server("http://a", "three")).unwrap(), first);
        let servers = store.servers().unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].token, "three");
        assert!(!format!("{:?}", servers[0]).contains("three"));
        store.remove_server(first).unwrap();
        assert_eq!(store.servers().unwrap()[0].id, second);
    }

    #[test]
    fn tv_sources_keep_their_order_and_refresh_in_place() {
        let store = Store::in_memory().unwrap();
        let source = |url: &str, playlist: &str| TvSource {
            url: url.into(),
            name: "name".into(),
            playlist: playlist.into(),
        };
        store.save_tv_source(&source("http://a/list.m3u", "one")).unwrap();
        store.save_tv_source(&source("http://b/list.m3u", "two")).unwrap();
        store.save_tv_source(&source("http://a/list.m3u", "three")).unwrap();
        let sources = store.tv_sources().unwrap();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].url, "http://a/list.m3u");
        assert_eq!(sources[0].playlist, "three");
        store.remove_tv_source("http://a/list.m3u").unwrap();
        assert_eq!(store.tv_sources().unwrap()[0].url, "http://b/list.m3u");
    }

    #[test]
    fn watchlist_is_newest_first_and_saving_twice_keeps_one_entry() {
        let store = Store::in_memory().unwrap();
        let saved = |id: &str, title: &str, poster: Option<&str>, at: i64| Saved {
            kind: "movie".into(),
            title_id: id.into(),
            title: title.into(),
            year: "2026".into(),
            poster: poster.map(Into::into),
            added_at: at,
        };
        assert!(!store.in_watchlist("movie", "a").unwrap());
        store.add_to_watchlist(&saved("a", "First", Some("https://example.org/a.jpg"), 1)).unwrap();
        store.add_to_watchlist(&saved("b", "Second", None, 2)).unwrap();
        // Saved again later, renamed and without a poster.
        store.add_to_watchlist(&saved("a", "First (renamed)", None, 9)).unwrap();
        assert!(store.in_watchlist("movie", "a").unwrap());
        assert!(!store.in_watchlist("series", "a").unwrap());

        let list = store.watchlist().unwrap();
        let titles: Vec<&str> = list.iter().map(|saved| saved.title.as_str()).collect();
        assert_eq!(titles, ["Second", "First (renamed)"]);
        assert_eq!(list[1].poster.as_deref(), Some("https://example.org/a.jpg"));
        assert_eq!(list[1].added_at, 1);

        store.remove_from_watchlist("movie", "b").unwrap();
        assert_eq!(store.watchlist().unwrap().len(), 1);
    }

    #[test]
    fn an_older_database_gains_the_addons_table() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute("INSERT INTO kv VALUES ('k', '\"v\"')", []).unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.get::<String>("k").unwrap().as_deref(), Some("v"));
        assert!(store.addons().unwrap().is_empty());
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
