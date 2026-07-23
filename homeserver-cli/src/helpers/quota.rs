use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(untagged)]
pub enum Quota {
    Limit(u64),
    Unlimited(UnlimitedTag),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum UnlimitedTag {
    Unlimited,
}

impl fmt::Display for Quota {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Quota::Limit(v) => write!(f, "{}", v),
            Quota::Unlimited(_) => write!(f, "unlimited"),
        }
    }
}

impl FromStr for Quota {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("unlimited") {
            Ok(Quota::Unlimited(UnlimitedTag::Unlimited))
        } else {
            s.parse::<u64>()
                .map(Quota::Limit)
                .map_err(|_| format!("expected a number or 'unlimited', got '{s}'"))
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RateLimit(String);

impl fmt::Display for RateLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for RateLimit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_ascii_lowercase();

        if normalized == "unlimited" {
            return Ok(RateLimit(normalized));
        }

        // expected shape: <number><unit>/<period>, e.g. 100mb/s
        let (amount_part, period) = normalized.split_once('/').ok_or_else(|| err_msg(s))?;

        let unit_start = amount_part
            .find(|c: char| !c.is_ascii_digit())
            .ok_or_else(|| err_msg(s))?;
        let (number, unit) = amount_part.split_at(unit_start);

        if number.is_empty() || number.parse::<u32>().is_err() {
            return Err(err_msg(s));
        }
        if !matches!(unit, "kb" | "mb" | "gb") {
            return Err(err_msg(s));
        }
        if !matches!(period, "s" | "m" | "h" | "d") {
            return Err(err_msg(s));
        }

        Ok(RateLimit(normalized))
    }
}

fn err_msg(s: &str) -> String {
    format!(
        "invalid rate '{s}': expected <number><kb|mb|gb>/<s|m|h|d> (e.g. 100mb/s) or 'unlimited'"
    )
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserQuota {
    pub effective: QuotaValues,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QuotaValues {
    pub storage_quota_mb: Quota,
    pub rate_read: String,
    pub rate_write: String,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct QuotaUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_quota_mb: Option<Quota>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_read: Option<RateLimit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_write: Option<RateLimit>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quota(s: &str) -> Result<Quota, String> {
        s.parse()
    }

    fn rate(s: &str) -> Result<RateLimit, String> {
        s.parse()
    }

    // --- Quota::from_str ---

    #[test]
    fn quota_parses_number() {
        assert!(matches!(quota("500"), Ok(Quota::Limit(500))));
    }

    #[test]
    fn quota_parses_zero() {
        assert!(matches!(quota("0"), Ok(Quota::Limit(0))));
    }

    #[test]
    fn quota_parses_unlimited_lowercase() {
        assert!(matches!(quota("unlimited"), Ok(Quota::Unlimited(_))));
    }

    #[test]
    fn quota_parses_unlimited_uppercase() {
        assert!(matches!(quota("UNLIMITED"), Ok(Quota::Unlimited(_))));
    }

    #[test]
    fn quota_rejects_negative() {
        assert!(quota("-1").is_err());
    }

    #[test]
    fn quota_rejects_float() {
        assert!(quota("1.5").is_err());
    }

    #[test]
    fn quota_rejects_empty() {
        assert!(quota("").is_err());
    }

    #[test]
    fn quota_display_number() {
        assert_eq!(Quota::Limit(1024).to_string(), "1024");
    }

    #[test]
    fn quota_display_unlimited() {
        assert_eq!(Quota::Unlimited(UnlimitedTag::Unlimited).to_string(), "unlimited");
    }

    // --- RateLimit::from_str ---

    #[test]
    fn rate_parses_mb_per_second() {
        assert!(rate("100mb/s").is_ok());
    }

    #[test]
    fn rate_parses_kb_per_minute() {
        assert!(rate("512kb/m").is_ok());
    }

    #[test]
    fn rate_parses_gb_per_hour() {
        assert!(rate("1gb/h").is_ok());
    }

    #[test]
    fn rate_parses_day_period() {
        assert!(rate("10mb/d").is_ok());
    }

    #[test]
    fn rate_parses_unlimited() {
        assert!(rate("unlimited").is_ok());
    }

    #[test]
    fn rate_is_case_insensitive() {
        assert!(rate("100MB/S").is_ok());
    }

    #[test]
    fn rate_trims_whitespace() {
        assert!(rate("  100mb/s  ").is_ok());
    }

    #[test]
    fn rate_rejects_bare_b_unit() {
        assert!(rate("500b/s").is_err());
    }

    #[test]
    fn rate_rejects_invalid_period() {
        assert!(rate("100mb/w").is_err());
    }

    #[test]
    fn rate_rejects_missing_slash() {
        assert!(rate("100mbs").is_err());
    }

    #[test]
    fn rate_rejects_missing_number() {
        assert!(rate("mb/s").is_err());
    }

    #[test]
    fn rate_rejects_missing_unit() {
        assert!(rate("100/s").is_err());
    }

    #[test]
    fn rate_rejects_empty() {
        assert!(rate("").is_err());
    }

    #[test]
    fn rate_normalizes_to_lowercase() {
        let r = rate("100MB/S").unwrap();
        assert_eq!(r.to_string(), "100mb/s");
    }
}
