//! Internal database in [super::HomeserverCore]
mod db;
mod migrations;
mod sql_migrator;
pub mod tables;
pub use db::LmDB;
