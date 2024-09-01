use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    pub resource: String,
    pub abilities: Vec<Ability>,
}

impl Capability {
    /// Create a root [Capability] at the `/` path with all the available [PubkyAbility]
    pub fn root() -> Self {
        Capability {
            resource: "/".to_string(),
            abilities: vec![Ability::Read, Ability::Write],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ability {
    /// Can read the resource at the specified path (GET requests).
    Read,
    /// Can write to the resource at the specified path (PUT/POST/DELETE requests).
    Write,
    /// Unknown ability
    Unknown(char),
}

impl From<&Ability> for char {
    fn from(value: &Ability) -> Self {
        match value {
            Ability::Read => 'r',
            Ability::Write => 'w',
            Ability::Unknown(char) => char.to_owned(),
        }
    }
}

impl TryFrom<char> for Ability {
    type Error = Error;

    fn try_from(value: char) -> Result<Self, Error> {
        match value {
            'r' => Ok(Self::Read),
            'w' => Ok(Self::Write),
            _ => Err(Error::InvalidAbility),
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
        write!(
            f,
            "{}:{}",
            self.resource,
            self.abilities.iter().map(char::from).collect::<String>()
        )
    }
}

impl TryFrom<&str> for Capability {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        if value.matches(':').count() != 1 {
            return Err(Error::InvalidFormat);
        }

        if !value.starts_with('/') {
            return Err(Error::InvalidResource);
        }

        let abilities_str = value.rsplit(':').next().unwrap_or("");

        let mut abilities = Vec::new();

        for char in abilities_str.chars() {
            let ability = Ability::try_from(char)?;

            match abilities.binary_search_by(|element| char::from(element).cmp(&char)) {
                Ok(_) => {}
                Err(index) => {
                    abilities.insert(index, ability);
                }
            }
        }

        let resource = value[0..value.len() - abilities_str.len() - 1].to_string();

        Ok(Capability {
            resource,
            abilities,
        })
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum Error {
    #[error("Capability: Invalid resource path: does not start with `/`")]
    InvalidResource,
    #[error("Capability: Invalid format should be <resource>:<abilities>")]
    InvalidFormat,
    #[error("Capability: Invalid Ability")]
    InvalidAbility,
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
        let cap = Capability {
            resource: "/pub/pubky.app/".to_string(),
            abilities: vec![Ability::Read, Ability::Write],
        };

        // Read and write withing directory `/pub/pubky.app/`.
        let expected_string = "/pub/pubky.app/:rw";

        assert_eq!(cap.to_string(), expected_string);

        assert_eq!(Capability::try_from(expected_string), Ok(cap))
    }
}
