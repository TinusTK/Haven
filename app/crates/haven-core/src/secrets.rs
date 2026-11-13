use std::{collections::HashMap, fmt, path::Path};

use crate::{Error, Result};

/// API keys for the services Haven talks to, read from an env-style file
/// (`KEY=value` lines, `#` comments). The file lives outside git.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    pub tmdb_api_key: Option<String>,
    pub trakt_client_id: Option<String>,
    pub trakt_client_secret: Option<String>,
    pub simkl_client_id: Option<String>,
}

impl Secrets {
    /// A missing file is not an error: it gives empty secrets.
    pub fn from_file(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(Error::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    pub fn parse(text: &str) -> Self {
        let mut map: HashMap<&str, &str> = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            if let Some((key, value)) = line.split_once('=') {
                map.insert(key.trim(), unquote(value.trim()));
            }
        }
        let take = |key: &str| {
            map.get(key)
                .filter(|v| !v.is_empty())
                .map(|v| (*v).to_owned())
        };
        Self {
            tmdb_api_key: take("TMDB_API_KEY"),
            trakt_client_id: take("TRAKT_CLIENT_ID"),
            trakt_client_secret: take("TRAKT_CLIENT_SECRET"),
            simkl_client_id: take("SIMKL_CLIENT_ID"),
        }
    }
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|v| v.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// Never print the values, only which ones are set.
impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let set = |v: &Option<String>| if v.is_some() { "set" } else { "missing" };
        f.debug_struct("Secrets")
            .field("tmdb_api_key", &set(&self.tmdb_api_key))
            .field("trakt_client_id", &set(&self.trakt_client_id))
            .field("trakt_client_secret", &set(&self.trakt_client_secret))
            .field("simkl_client_id", &set(&self.simkl_client_id))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comments_quotes_and_export() {
        let secrets = Secrets::parse(
            "# keys\nTMDB_API_KEY = abc123\nexport TRAKT_CLIENT_ID=\"id\"\n\
             TRAKT_CLIENT_SECRET='s=1'\nSIMKL_CLIENT_ID=\nUNKNOWN=x\nnot a line\n",
        );
        assert_eq!(secrets.tmdb_api_key.as_deref(), Some("abc123"));
        assert_eq!(secrets.trakt_client_id.as_deref(), Some("id"));
        assert_eq!(secrets.trakt_client_secret.as_deref(), Some("s=1"));
        assert_eq!(secrets.simkl_client_id, None);
    }

    #[test]
    fn missing_file_gives_empty_secrets() {
        let tmp = tempfile::tempdir().unwrap();
        let secrets = Secrets::from_file(&tmp.path().join("none.env")).unwrap();
        assert_eq!(secrets, Secrets::default());
    }

    #[test]
    fn debug_output_hides_values() {
        let secrets = Secrets::parse("TMDB_API_KEY=topsecret");
        let shown = format!("{secrets:?}");
        assert!(!shown.contains("topsecret"));
        assert!(shown.contains("set"));
    }
}
