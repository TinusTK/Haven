//! The shapes every source of titles shares: posters for a shelf, a
//! title's details with its episodes, and the ways to watch it. Media
//! servers, the built-in catalogues and the Hub all speak in these, so the
//! rest of Haven does not care where a title came from.
//!
//! The JSON shape is the widely used one Cinemeta answers in.

#[doc(hidden)]
pub mod lenient;
mod catalog;
mod client;
mod meta;
mod source;
mod stream;
mod url;

pub use meta::{
    MetaDetail, MetaPreview, Video, is_release_name, parse_catalog, parse_meta, title_key,
    without_repeats,
};
pub use catalog::{Catalog, Extra, parse_catalogs};
pub use client::Catalogues;
pub use source::{CINEMETA_NAME, CINEMETA_URL, Serves, Source};
pub use stream::{Stream, StreamKind, parse_streams};
pub use url::{catalog_url, resource_url, resource_url_with};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("\"{0}\" is not a web address")]
    InvalidAddress(String),
    #[error(transparent)]
    Http(#[from] haven_core::http::HttpError),
    #[error("the answer was not text")]
    NotText,
    #[error("the answer could not be read: {0}")]
    BadResponse(serde_json::Error),
    #[error("this title is not known there")]
    UnknownTitle,
}

pub type Result<T> = std::result::Result<T, Error>;
