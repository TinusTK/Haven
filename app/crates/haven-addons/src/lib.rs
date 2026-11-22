//! Add-ons: web services that give Haven catalogues, details and streams.
//!
//! Haven speaks the public Stremio add-on protocol. An add-on is an
//! address that serves a `manifest.json` describing what it offers; every
//! other resource is a JSON file next to it, for example
//! `catalog/<type>/<id>.json`, `meta/<type>/<id>.json` and
//! `stream/<type>/<id>.json`.

mod client;
mod manifest;
mod url;

pub use client::Client;
pub use manifest::{Manifest, Resource, parse_manifest};
pub use haven_titles::{
    Catalog, Extra, MetaDetail, MetaPreview, Source, Stream, StreamKind, Video, catalog_url,
    is_release_name, resource_url, resource_url_with, title_key, without_repeats,
};

/// Reads a catalogue page an add-on sent.
pub fn parse_catalog(json: &str) -> Result<Vec<MetaPreview>> {
    Ok(haven_titles::parse_catalog(json)?)
}

/// Reads the details an add-on sent for one title.
pub fn parse_meta(json: &str) -> Result<MetaDetail> {
    Ok(haven_titles::parse_meta(json)?)
}

/// Reads the streams an add-on offered.
pub fn parse_streams(json: &str) -> Result<Vec<Stream>> {
    Ok(haven_titles::parse_streams(json)?)
}
pub use url::manifest_url;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("\"{0}\" is not an add-on address")]
    InvalidAddress(String),
    #[error(transparent)]
    Http(#[from] haven_core::http::HttpError),
    #[error("the add-on's manifest could not be read: {0}")]
    BadManifest(serde_json::Error),
    #[error("the add-on's manifest has no {0}")]
    IncompleteManifest(&'static str),
    #[error("the add-on's answer could not be read: {0}")]
    BadResponse(serde_json::Error),
    #[error("the add-on's answer was not text")]
    NotText,
    #[error("the add-on does not know this title")]
    UnknownTitle,
}

impl From<haven_titles::Error> for Error {
    fn from(error: haven_titles::Error) -> Self {
        match error {
            haven_titles::Error::BadResponse(e) => Error::BadResponse(e),
            haven_titles::Error::UnknownTitle => Error::UnknownTitle,
            haven_titles::Error::InvalidAddress(address) => Error::InvalidAddress(address),
            haven_titles::Error::Http(e) => Error::Http(e),
            haven_titles::Error::NotText => Error::NotText,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
