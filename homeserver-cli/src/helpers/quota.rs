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
        "invalid rate '{s}': expected <number><b|kb|mb|gb>/<s|m|h> (e.g. 100mb/s) or 'unlimited'"
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
