//! Internal database in [super::HomeserverCore]
mod db;
mod migrations;
mod sql_migrator;
pub mod tables;
pub use db::LmDB;
pub use sql_migrator::{migrate_lmdb_to_sql, is_migration_needed};
