//! HTTP client for the `rust-gvm-api` REST gateway.

pub mod auth;
pub mod client;
pub mod error;
pub mod jobs;
pub mod models;
pub mod sessions;

pub use client::{Artifact, GatewayClient};
pub use error::{GatewayError, ProblemDetail};
pub use jobs::JobSessions;
pub use sessions::SessionPool;
