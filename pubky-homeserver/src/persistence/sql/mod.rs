//! PostgreSQL persistence.
//!
//! Manages the connection pool ([`SqlDb`]), schema migrations ([`Migrator`]),
//! and entity repositories for users, sessions, entries, events, and signup codes.
//! The [`UnifiedExecutor`] abstraction allows repository methods to work with
//! both pooled connections and explicit transactions.

mod connection_string;
mod entities;
mod migration;
mod migrations;
mod migrator;
mod sql_db;
mod unified_executor;

pub use connection_string::ConnectionString;
pub use entities::*;
pub use migrator::Migrator;
pub use sql_db::SqlDb;
pub(crate) use unified_executor::uexecutor;
pub(crate) use unified_executor::UnifiedExecutor;
