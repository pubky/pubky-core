use std::{
    fmt::{self, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

use super::validate_domain::validate_domain_str;

/// A domain and port pair.
#[derive(Debug, Clone, PartialEq)]
pub struct DomainPort {
    pub domain: String,
    pub port: u16,
}

impl TryFrom<&str> for DomainPort {
    type Error = anyhow::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_str(s)
    }
}

impl TryFrom<String> for DomainPort {
    type Error = anyhow::Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::from_str(&s)
    }
}

impl From<DomainPort> for String {
    fn from(domain_port: DomainPort) -> Self {
        format!("{}:{}", domain_port.domain, domain_port.port)
    }
}

impl FromStr for DomainPort {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid domain:port format. Expected 'domain:port'"
            ));
        }
        let part0 = parts[0];

        let domain = validate_domain_str(part0)?;
        let port = parts[1].parse::<u16>()?;

        Ok(Self { domain, port })
    }
}

impl Display for DomainPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.domain, self.port)
    }
}

impl Serialize for DomainPort {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DomainPort {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_port_from_str() {
        let domain_port = DomainPort::from_str("example.com:6286").unwrap();
        assert_eq!(domain_port.domain, "example.com");
        assert_eq!(domain_port.port, 6286);
    }

    #[test]
    fn test_domain_port_from_str_invalid() {
        let domain_port = DomainPort::from_str("example.com");
        assert!(domain_port.is_err());
    }
}
