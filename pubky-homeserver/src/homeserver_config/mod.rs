//! Server data directory and configuration.
//!
//! Manages the on-disk data directory (default `~/.pubky/`) which contains the
//! server keypair, `config.toml`, and file storage. [`ConfigToml`] is loaded by
//! merging embedded defaults with user overrides and controls all server behavior
//! (listen addresses, signup mode, storage backend, rate limits, logging, etc.).

mod config_toml;
mod domain;
mod domain_port;
mod homeserver_paths;
#[cfg(any(test, feature = "testing"))]
mod mock_setup_source;
/// Quota configuration for the TomlConfig.
pub mod quota_config;
mod setup_source;
mod signup_mode;
/// Opendal config for the TomlConfig.
pub mod storage_config;

mod log_level;
pub use config_toml::{AdminToml, ConfigReadError, ConfigToml, LoggingToml, MetricsToml};
pub use domain::Domain;
pub use domain_port::DomainPort;
pub use homeserver_paths::HomeserverPaths;
#[cfg(any(test, feature = "testing"))]
pub use mock_setup_source::MockSetupSource;
pub use setup_source::SetupSource;
pub use signup_mode::SignupMode;
