//! Shared foundations for Haven: where files live, the local database,
//! user settings and API secrets. No UI code belongs here.

pub mod error;
pub mod paths;
pub mod secrets;
pub mod settings;
pub mod storage;

pub use error::{Error, Result};
