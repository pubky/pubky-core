use std::fmt;
use std::str::FromStr;

/// The key to limit the quota on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LimitKey {
    /// Limit on the user id    
    User,
    /// Limit on the ip address
    Ip,
}

impl fmt::Display for LimitKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            LimitKey::User => "user",
            LimitKey::Ip => "ip",
        })
    }
}

impl FromStr for LimitKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(LimitKey::User),
            "ip" => Ok(LimitKey::Ip),
            _ => Err(format!("Invalid limit key: {}", s)),
        }
    }
}

impl serde::Serialize for LimitKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        serializer.serialize_str(self.to_string().as_str())
    }
}

impl<'de> serde::Deserialize<'de> for LimitKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        LimitKey::from_str(&s).map_err(serde::de::Error::custom)
    }
}