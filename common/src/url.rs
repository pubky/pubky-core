use std::fmt::Display;

use pkarr::PublicKey;

use crate::{Error, Result};

pub struct PkUrl {
    authority: PublicKey,
    path: Option<String>,
}

impl PkUrl {
    pub fn parse(url: &str) -> Result<Self> {
        if url.starts_with("pk:") {
            return Err(Error::Generic("Expected a pk: url".to_string()));
        };

        let mut authority: Option<PublicKey> = None;
        let mut path = None;

        // TODO: handle Query parameters
        if let Some((first, second)) = url.split_once("/") {
            // We depend on Pkarr's implementation of parsing the authority part as a [PublicKey]
            authority = first.try_into().ok();

            path = Some(second.to_string());
        };

        if let Some(authority) = authority {
            return Ok(Self { authority, path });
        }

        Err(Error::Generic("Invalid authority".to_string()))
    }

    // === Getters ===

    pub fn authority(&self) -> &PublicKey {
        &self.authority
    }

    pub fn path(&self) -> String {
        self.path.clone().unwrap_or("".to_string())
    }
}

impl Display for PkUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "pk:{}{}",
            self.authority,
            if self.path.is_some() {
                format!("/{}", self.path.clone().unwrap_or("".to_string()))
            } else {
                "".to_string()
            }
        ))
    }
}
