// Route handlers return `Result<_, Response>`: the error arm is a finished
// HTTP response by design, so its size is not a concern.
#![allow(clippy::result_large_err)]

pub mod asset_hashes;
mod config;
mod daily_tasks;
mod domain;
mod file_utils;
mod invoice_watcher;
mod lightning;
pub mod metrics;
mod nostr_extractor;
mod routes;
mod secrets;
mod startup;
mod templates;

pub use config::*;
pub use daily_tasks::*;
pub use domain::*;
pub use invoice_watcher::*;
pub use lightning::*;
pub use routes::*;
pub use secrets::{get_key, SecretKeyHandler};
pub use startup::*;
