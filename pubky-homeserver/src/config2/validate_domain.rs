use serde::{Deserialize, Deserializer};
use std::result::Result;

/// Validate a domain name according to RFC 1123
pub fn validate_domain<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let domain: Option<String> = Option::deserialize(deserializer)?;
    
    if let Some(ref domain) = domain {
        // Check if it's a valid hostname according to RFC 1123
        if !domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') {
            return Err(serde::de::Error::custom(format!(
                "Invalid domain '{}': contains invalid characters. Only alphanumeric characters, hyphens, and dots are allowed.",
                domain
            )));
        }

        // Check if it starts or ends with a dot or hyphen
        if domain.starts_with('.') || domain.ends_with('.') || 
           domain.starts_with('-') || domain.ends_with('-') {
            return Err(serde::de::Error::custom(format!(
                "Invalid domain '{}': cannot start or end with a dot or hyphen",
                domain
            )));
        }

        // Check if it contains consecutive dots or hyphens
        if domain.contains("..") || domain.contains("--") {
            return Err(serde::de::Error::custom(format!(
                "Invalid domain '{}': cannot contain consecutive dots or hyphens",
                domain
            )));
        }

        // Check if it's not too long (max 253 characters per RFC 1035)
        if domain.len() > 253 {
            return Err(serde::de::Error::custom(format!(
                "Invalid domain '{}': exceeds maximum length of 253 characters",
                domain
            )));
        }

        // Check if each label is not too long (max 63 characters per RFC 1035)
        for label in domain.split('.') {
            if label.len() > 63 {
                return Err(serde::de::Error::custom(format!(
                    "Invalid domain '{}': label '{}' exceeds maximum length of 63 characters",
                    domain, label
                )));
            }
        }
    }
    
    Ok(domain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[allow(unused)]
    #[derive(Debug, Deserialize)]
    struct TestConfig {
        #[serde(deserialize_with = "validate_domain")]
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
            ("domain--name.com", "contains consecutive hyphens"),
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
            assert!(result.is_err(), "Domain '{}' should be invalid: {}", domain, reason);
        }
    }
}