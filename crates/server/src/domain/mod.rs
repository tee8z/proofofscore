mod games;
mod ledger;
mod payments;
mod users;

pub use games::*;
pub use ledger::*;
pub use payments::*;
pub use users::*;

use axum::response::{IntoResponse, Response};
use hyper::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Thread error: {0}")]
    Thread(String),
}

pub fn map_error(err: Error) -> Response {
    match err {
        Error::NotFound(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
        Error::InvalidInput(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
        Error::Authentication(msg) => (StatusCode::UNAUTHORIZED, msg).into_response(),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response(),
    }
}
