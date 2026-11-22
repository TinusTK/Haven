use haven_core::http::get_bytes;

use crate::{
    Catalog, Error, MetaDetail, MetaPreview, Result, catalog_url, parse_catalog, parse_meta,
    resource_url, resource_url_with,
};

const DESCRIPTION_LIMIT: usize = 1024 * 1024;
const ANSWER_LIMIT: usize = 8 * 1024 * 1024;

/// Reads catalogues and details from a catalogue service over HTTP.
#[derive(Clone)]
pub struct Catalogues {
    http: reqwest::Client,
}

impl Catalogues {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    async fn text(&self, url: &str, limit: usize) -> Result<String> {
        let body = get_bytes(&self.http, url, limit).await?;
        String::from_utf8(body).map_err(|_| Error::NotText)
    }

    /// The service's own description as raw JSON.
    pub async fn description_json(&self, url: &str) -> Result<String> {
        self.text(url, DESCRIPTION_LIMIT).await
    }

    /// The first page of a catalogue, or of its search results.
    pub async fn catalog(
        &self,
        source_url: &str,
        catalog: &Catalog,
        search: Option<&str>,
    ) -> Result<Vec<MetaPreview>> {
        let url = catalog_url(source_url, catalog, search)?;
        parse_catalog(&self.text(&url, ANSWER_LIMIT).await?)
    }

    /// A page of a catalogue: narrowed to `genre` if given, starting
    /// `skip` titles in.
    pub async fn catalog_page(
        &self,
        source_url: &str,
        catalog: &Catalog,
        genre: Option<&str>,
        skip: usize,
    ) -> Result<Vec<MetaPreview>> {
        let skip = skip.to_string();
        let mut extras = Vec::new();
        if let Some(genre) = genre {
            extras.push(("genre", genre));
        }
        if skip != "0" {
            extras.push(("skip", skip.as_str()));
        }
        let url = resource_url_with(source_url, "catalog", &catalog.kind, &catalog.id, &extras)?;
        parse_catalog(&self.text(&url, ANSWER_LIMIT).await?)
    }

    /// Everything the source knows about one title.
    pub async fn meta(&self, source_url: &str, kind: &str, id: &str) -> Result<MetaDetail> {
        let url = resource_url(source_url, "meta", kind, id, None)?;
        parse_meta(&self.text(&url, ANSWER_LIMIT).await?)
    }
}
