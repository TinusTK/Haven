//! Add-ons: web services that give Haven catalogues, details and streams.
//!
//! Haven speaks the public Stremio add-on protocol. An add-on is an
//! address that serves a `manifest.json` describing what it offers; every
//! other resource is a JSON file next to it, for example
//! `catalog/<type>/<id>.json`, `meta/<type>/<id>.json` and
//! `stream/<type>/<id>.json`.

mod client;
mod lenient;
mod manifest;
mod meta;
mod stream;
mod url;

pub use client::Client;
pub use manifest::{Catalog, Extra, Manifest, Resource, parse_manifest};
pub use meta::{MetaDetail, MetaPreview, Video, parse_catalog, parse_meta};
pub use stream::{Stream, StreamKind, parse_streams};
pub use url::{catalog_url, manifest_url, resource_url};

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

pub type Result<T> = std::result::Result<T, Error>;
