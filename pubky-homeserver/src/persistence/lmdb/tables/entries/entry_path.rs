use std::str::FromStr;
use pkarr::PublicKey;

use super::WebDavPath;

#[derive(thiserror::Error, Debug)]
pub enum EntryPathError {
    #[error("{0}")]
    Invalid(String),
    #[error("Failed to parse webdav path: {0}")]
    InvalidWebdavPath(anyhow::Error),
    #[error("Failed to parse pubkey: {0}")]
    InvalidPubkey(pkarr::errors::PublicKeyError),
}

/// A path to an entry.
///
/// The path as a string is used to identify the entry.
struct EntryPath {
    pubkey: PublicKey,
    path: WebDavPath,
}

impl EntryPath {
    pub fn new(pubkey: PublicKey, path: WebDavPath) -> Self {
        Self {
            pubkey,
            path,
        }
    }

    pub fn pubkey(&self) -> &PublicKey {
        &self.pubkey
    }

    pub fn path(&self) -> &WebDavPath {
        &self.path
    }
}

impl std::fmt::Display for EntryPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.pubkey, self.path)
    }
}

impl FromStr for EntryPath {
    type Err = EntryPathError;

    fn from_str(s: &str) -> Result<Self, EntryPathError> {
        let first_slash_index = s.find('/').ok_or(EntryPathError::Invalid("Missing '/'".to_string()))?;
        let (pubkey, path) = match s.split_at_checked(first_slash_index) {
            Some((pubkey, path)) => (pubkey, path),
            None => return Err(EntryPathError::Invalid("Missing '/'".to_string())),
        };
        let pubkey = PublicKey::from_str(pubkey).map_err(EntryPathError::InvalidPubkey)?;
        let webdav_path = WebDavPath::new(path).map_err(EntryPathError::InvalidWebdavPath)?;
        Ok(Self::new(pubkey, webdav_path))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_path_from_str() {
        let pubkey = PublicKey::from_str("8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo").unwrap();
        let path = "/folder/file.txt";
        let key = format!("{pubkey}{path}");
        let entry_path = EntryPath::from_str(&key).unwrap();
        assert_eq!(entry_path.to_string(), key);
    }


}
