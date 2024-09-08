use std::fmt::Display;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    pub scope: String,
    pub actions: Vec<Action>,
}

impl Capability {
    /// Create a root [Capability] at the `/` path with all the available [PubkyAbility]
    pub fn root() -> Self {
        Capability {
            scope: "/".to_string(),
            actions: vec![Action::Read, Action::Write],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Can read the scope at the specified path (GET requests).
    Read,
    /// Can write to the scope at the specified path (PUT/POST/DELETE requests).
    Write,
    /// Unknown ability
    Unknown(char),
}

impl From<&Action> for char {
    fn from(value: &Action) -> Self {
        match value {
            Action::Read => 'r',
            Action::Write => 'w',
            Action::Unknown(char) => char.to_owned(),
        }
    }
}

impl TryFrom<char> for Action {
    type Error = Error;

    fn try_from(value: char) -> Result<Self, Error> {
        match value {
            'r' => Ok(Self::Read),
            'w' => Ok(Self::Write),
            _ => Err(Error::InvalidAction),
        }
    }
}

impl Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}",
            self.scope,
            self.actions.iter().map(char::from).collect::<String>()
        )
    }
}

impl TryFrom<String> for Capability {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        value.as_str().try_into()
    }
}

impl TryFrom<&str> for Capability {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Error> {
        if value.matches(':').count() != 1 {
            return Err(Error::InvalidFormat);
        }

        if !value.starts_with('/') {
            return Err(Error::InvalidScope);
        }

        let actions_str = value.rsplit(':').next().unwrap_or("");

        let mut actions = Vec::new();

        for char in actions_str.chars() {
            let ability = Action::try_from(char)?;

            match actions.binary_search_by(|element| char::from(element).cmp(&char)) {
                Ok(_) => {}
                Err(index) => {
                    actions.insert(index, ability);
                }
            }
        }

        let scope = value[0..value.len() - actions_str.len() - 1].to_string();

        Ok(Capability { scope, actions })
    }
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

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum Error {
    #[error("Capability: Invalid scope: does not start with `/`")]
    InvalidScope,
    #[error("Capability: Invalid format should be <scope>:<abilities>")]
    InvalidFormat,
    #[error("Capability: Invalid Action")]
    InvalidAction,
    #[error("Capabilities: Invalid capabilities format")]
    InvalidCapabilities,
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
/// A wrapper around `Vec<Capability>` to enable serialization without
/// a varint. Useful when [Capabilities] are at the end of a struct.
pub struct Capabilities(pub Vec<Capability>);

impl Capabilities {
    pub fn contains(&self, capability: &Capability) -> bool {
        self.0.contains(capability)
    }
}

impl From<Vec<Capability>> for Capabilities {
    fn from(value: Vec<Capability>) -> Self {
        Self(value)
    }
}

impl From<Capabilities> for Vec<Capability> {
    fn from(value: Capabilities) -> Self {
        value.0
    }
}

impl TryFrom<&str> for Capabilities {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut caps = vec![];

        for s in value.split(',') {
            if let Ok(cap) = Capability::try_from(s) {
                caps.push(cap);
            };
        }

        Ok(Capabilities(caps))
    }
}

impl Display for Capabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string = self
            .0
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(",");

        write!(f, "{}", string)
    }
}

impl Serialize for Capabilities {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Capabilities {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string: String = Deserialize::deserialize(deserializer)?;

        let mut caps = vec![];

        for s in string.split(',') {
            if let Ok(cap) = Capability::try_from(s) {
                caps.push(cap);
            };
        }

        Ok(Capabilities(caps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pubky_caps() {
        let cap = Capability {
            scope: "/pub/pubky.app/".to_string(),
            actions: vec![Action::Read, Action::Write],
        };

        // Read and write withing directory `/pub/pubky.app/`.
        let expected_string = "/pub/pubky.app/:rw";

        assert_eq!(cap.to_string(), expected_string);

        assert_eq!(Capability::try_from(expected_string), Ok(cap))
    }
}
