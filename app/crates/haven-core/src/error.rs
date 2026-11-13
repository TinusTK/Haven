use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not find a home folder to keep Haven's data in")]
    NoHomeFolder,
    #[error("could not use {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("stored data could not be read: {0}")]
    Json(#[from] serde_json::Error),
    #[error("this database was made by a newer Haven (schema {found}, this build knows {known})")]
    DatabaseTooNew { found: u32, known: u32 },
}

pub type Result<T> = std::result::Result<T, Error>;
