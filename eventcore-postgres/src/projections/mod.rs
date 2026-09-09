//! Transactional projection delivery support for PostgreSQL.

mod migration;
mod source;

pub use source::{PostgresProjectionSource, PostgresProjectionSourceError};
