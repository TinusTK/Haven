use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::{Error, Result};

/// The folders Haven keeps its files in.
#[derive(Debug, Clone)]
pub struct AppPaths {
    data: PathBuf,
}

impl AppPaths {
    /// The standard per-user location for this OS, created if missing.
    pub fn discover() -> Result<Self> {
        let dirs = ProjectDirs::from("", "", "Haven").ok_or(Error::NoHomeFolder)?;
        Self::at(dirs.data_dir())
    }

    /// A specific folder, created if missing (also used by tests).
    pub fn at(data: impl Into<PathBuf>) -> Result<Self> {
        let data = data.into();
        std::fs::create_dir_all(&data).map_err(|source| Error::Io {
            path: data.clone(),
            source,
        })?;
        Ok(Self { data })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    pub fn database(&self) -> PathBuf {
        self.data.join("haven.sqlite3")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_the_folder_and_names_the_database() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(tmp.path().join("nested/haven")).unwrap();
        assert!(paths.data_dir().is_dir());
        assert_eq!(paths.database().file_name().unwrap(), "haven.sqlite3");
    }
}
