//! Shared foundations for Haven: where files live, the local database,
//! user settings, API secrets and the shared HTTP client. No UI code belongs here.

pub mod diag;
pub mod error;
pub mod http;
pub mod paths;
pub mod secrets;
pub mod settings;
pub mod storage;

pub use error::{Error, Result};
