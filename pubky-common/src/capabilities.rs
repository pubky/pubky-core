use std::fmt::Display;

use serde::{Deserialize, Serialize};

const PUBKY_CAP_PREFIX: &str = "pk!";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// Pubky Homeserver's capabilities
    Pubky(PubkyCap),
    Unknown(String),
}

impl Capability {
    /// Create a [PubkyCap] at the root path `/` with all the available [PubkyAbility]
    pub fn pubky_root() -> Self {
        Capability::Pubky(PubkyCap {
            path: "/".to_string(),
            abilities: vec![PubkyAbility::Read, PubkyAbility::Write],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubkyCap {
    pub path: String,
    pub abilities: Vec<PubkyAbility>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PubkyAbility {
    /// Can read the resource at the specified path (GET requests).
    Read,
    /// Can write to the resource at the specified path (PUT/POST/DELETE requests).
    Write,
}

impl From<&PubkyAbility> for char {
    fn from(value: &PubkyAbility) -> Self {
        match value {
            PubkyAbility::Read => 'r',
            PubkyAbility::Write => 'w',
        }
    }
}

impl TryFrom<char> for PubkyAbility {
    type Error = Error;

    fn try_from(value: char) -> Result<Self, Error> {
        match value {
            'r' => Ok(Self::Read),
            'w' => Ok(Self::Write),
            _ => Err(Error::InvalidPubkyAbility),
        }
    }
}

impl TryFrom<String> for Capability {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        value.as_str().try_into()
    }
}

impl Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pubky(cap) => write!(
                f,
                "{}{}:{}",
                PUBKY_CAP_PREFIX,
                cap.path,
                cap.abilities.iter().map(char::from).collect::<String>()
            ),
            Self::Unknown(string) => write!(f, "{string}"),
        }
    }
}

impl TryFrom<&str> for Capability {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        if value.starts_with(PUBKY_CAP_PREFIX) {
            let mut rsplit = value.rsplit(':');

            let mut abilities = Vec::new();

            for char in rsplit
                .next()
                .ok_or(Error::MissingField("abilities"))?
                .chars()
            {
                let ability = PubkyAbility::try_from(char)?;

                match abilities.binary_search_by(|element| char::from(element).cmp(&char)) {
                    Ok(_) => {}
                    Err(index) => {
                        abilities.insert(index, ability);
                    }
                }
            }

            let path = rsplit.next().ok_or(Error::MissingField("path"))?[PUBKY_CAP_PREFIX.len()..]
                .to_string();

            if !path.starts_with('/') {
                return Err(Error::InvalidPath);
            }

            return Ok(Capability::Pubky(PubkyCap { path, abilities }));
        }

        Ok(Capability::Unknown(value.to_string()))
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum Error {
    #[error("PubkyCap: Missing field {0}")]
    MissingField(&'static str),
    #[error("PubkyCap: InvalidPath does not start with `/`")]
    InvalidPath,
    #[error("Invalid PubkyAbility")]
    InvalidPubkyAbility,
}

impl Serialize for Capability {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let string = self.to_string();

        string.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string: String = Deserialize::deserialize(deserializer)?;

        string.try_into().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pubky_caps() {
        let cap = Capability::Pubky(PubkyCap {
            path: "/pub/pubky.app/".to_string(),
            abilities: vec![PubkyAbility::Read, PubkyAbility::Write],
        });

        // Read and write withing directory `/pub/pubky.app/`.
        let expected_string = "pk!/pub/pubky.app/:rw";

        assert_eq!(cap.to_string(), expected_string);

        assert_eq!(Capability::try_from(expected_string), Ok(cap))
    }
}
