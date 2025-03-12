//! Internal database in [super::HomeserverCore]
mod migrations;
mod db;
pub mod tables;
pub use db::DB;