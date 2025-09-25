//! Internal database in [super::HomeserverCore]
//! TODO: Remove this module after the migration is complete.
mod db;
mod migrations;
mod sql_migrator;
pub mod tables;
pub use db::LmDB;
pub use sql_migrator::{is_migration_needed, migrate_lmdb_to_sql};
