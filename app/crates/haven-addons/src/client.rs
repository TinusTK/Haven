use haven_core::http::get_bytes;

use crate::{
    Catalog, Error, MetaDetail, MetaPreview, Result, Stream, catalog_url, parse_catalog,
    parse_meta, parse_streams, resource_url,
};

const MANIFEST_LIMIT: usize = 1024 * 1024;
const ANSWER_LIMIT: usize = 8 * 1024 * 1024;

/// Talks to add-ons over HTTP.
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

    /// The first page of a catalogue, or of its search results.
    pub async fn catalog(
        &self,
        manifest_url: &str,
        catalog: &Catalog,
        search: Option<&str>,
    ) -> Result<Vec<MetaPreview>> {
        let url = catalog_url(manifest_url, catalog, search)?;
        parse_catalog(&self.text(&url, ANSWER_LIMIT).await?)
    }

    /// Everything the add-on knows about one title.
    pub async fn meta(&self, manifest_url: &str, kind: &str, id: &str) -> Result<MetaDetail> {
        let url = resource_url(manifest_url, "meta", kind, id, None)?;
        parse_meta(&self.text(&url, ANSWER_LIMIT).await?)
    }

    /// The add-on's streams for a film, or for one episode (`id` is then
    /// the episode's id).
    pub async fn streams(&self, manifest_url: &str, kind: &str, id: &str) -> Result<Vec<Stream>> {
        let url = resource_url(manifest_url, "stream", kind, id, None)?;
        parse_streams(&self.text(&url, ANSWER_LIMIT).await?)
    }
}
