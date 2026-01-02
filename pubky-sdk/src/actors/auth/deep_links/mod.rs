//! Deep link related module.
//! Contains the following:
//! - `SigninDeepLink` - A deep link for signing into a Pubky homeserver.
//! - `SignupDeepLink` - A deep link for signing up to a Pubky homeserver.
//! - `SeedExportDeepLink` - A deep link for exporting a user secret to a signer like Pubky Ring.
//! - `DeepLink` - A parsed Pubky deep link.
//! - `DeepLinkParseError` - Errors that can occur when parsing a deep link.
//! - `DEEP_LINK_SCHEMES` - Supported deep link schemes.
//!
//! A deep link is used either on a phone directly or in the browser as a QR code
//! to communicate with a Pubky Signer like Pubky Ring.

mod deep_link;
mod error;
mod seed_export;
mod signin;
mod signup;

/// Supported deep link schemes.
pub const DEEP_LINK_SCHEMES: [&str; 2] = ["pubkyauth", "pubkyring"];

pub use deep_link::DeepLink;
pub use error::DeepLinkParseError;
pub use seed_export::SeedExportDeepLink;
pub use signin::SigninDeepLink;
pub use signup::SignupDeepLink;
