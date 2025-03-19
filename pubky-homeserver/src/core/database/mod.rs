//! Internal database in [super::HomeserverCore]
mod db;
mod migrations;
pub mod tables;
pub use db::DB;
