use serde::{Deserialize, Serialize, Deserializer};
use std::fmt;
use std::str::FromStr;
use std::result::Result;


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

impl fmt::Display for DomainPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.domain, self.port)
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

/// Validate a domain name according to RFC 1123
pub fn validate_domain_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let domain: Option<String> = Option::deserialize(deserializer)?;

    if let Some(ref domain) = domain {
        let domain =
            validate_domain_str(domain).map_err(|e| serde::de::Error::custom(e.to_string()))?;
        Ok(Some(domain))
    } else {
        Ok(None)
    }
}

/// Validate a domain name according to RFC 1123
pub fn validate_domain_str(domain: &str) -> anyhow::Result<String> {
    // Check if it's a valid hostname according to RFC 1123
    if !hostname_validator::is_valid(domain) {
        return Err(anyhow::anyhow!(
            "Invalid domain '{}': is not a valid RFC 1123 hostname",
            domain
        ));
    }
    Ok(domain.to_string())
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

    #[allow(unused)]
    #[derive(Debug, Deserialize)]
    struct TestConfig {
        #[serde(deserialize_with = "validate_domain_opt")]
        domain: Option<String>,
    }

    #[test]
    fn test_domain_validation() {
        // Test valid domains
        let valid_domains = [
            "example.com",
            "sub.example.com",
            "a.b.c.d",
            "valid-domain.com",
            "valid.domain-name.com",
            "localhost",
            "test.local",
        ];

        for domain in valid_domains {
            let config = format!(
                r#"
                domain = "{}"
                "#,
                domain
            );
            let result = toml::from_str::<TestConfig>(&config);
            assert!(result.is_ok(), "Domain '{}' should be valid", domain);
        }

        // Test invalid domains
        let invalid_domains = [
            ("invalid@domain.com", "contains invalid characters"),
            ("domain..com", "contains consecutive dots"),
            (".domain.com", "starts with a dot"),
            ("domain.com.", "ends with a dot"),
            ("-domain.com", "starts with a hyphen"),
            ("domain.com-", "ends with a hyphen"),
        ];

        for (domain, reason) in invalid_domains {
            let config = format!(
                r#"
                domain = "{}"
                "#,
                domain
            );
            let result = toml::from_str::<TestConfig>(&config);
            assert!(
                result.is_err(),
                "Domain '{}' should be invalid: {}",
                domain,
                reason
            );
        }
    }
}
