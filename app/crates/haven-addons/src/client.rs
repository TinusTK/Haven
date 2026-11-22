use haven_core::http::get_bytes;

use crate::{Error, Result, Stream, parse_streams};

const MANIFEST_LIMIT: usize = 1024 * 1024;
const ANSWER_LIMIT: usize = 8 * 1024 * 1024;

/// Talks to add-ons over HTTP. Their catalogues and details are read with
/// [`haven_titles::Catalogues`], like the built-in catalogue's.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
}

impl Client {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    async fn text(&self, url: &str, limit: usize) -> Result<String> {
        let body = get_bytes(&self.http, url, limit).await?;
        String::from_utf8(body).map_err(|_| Error::NotText)
    }

    /// The add-on's manifest as the raw JSON it served (kept as is, so
    /// nothing the add-on declares is lost). Check it with
    /// [`crate::parse_manifest`].
    pub async fn manifest_json(&self, manifest_url: &str) -> Result<String> {
        self.text(manifest_url, MANIFEST_LIMIT).await
    }

    /// The add-on's streams for a film, or for one episode (`id` is then
    /// the episode's id).
    pub async fn streams(&self, manifest_url: &str, kind: &str, id: &str) -> Result<Vec<Stream>> {
        let url = haven_titles::resource_url(manifest_url, "stream", kind, id, None)?;
        parse_streams(&self.text(&url, ANSWER_LIMIT).await?)
    }
}
