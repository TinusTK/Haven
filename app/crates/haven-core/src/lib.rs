//! Shared foundations for Haven: where files live, the local database,
//! user settings, API secrets and the shared HTTP client. No UI code belongs here.

pub mod backup;
pub mod diag;
pub mod error;
pub mod http;
pub mod setup;
pub mod paths;
pub mod secrets;
pub mod settings;
pub mod storage;
pub mod update;

pub use error::{Error, Result};
