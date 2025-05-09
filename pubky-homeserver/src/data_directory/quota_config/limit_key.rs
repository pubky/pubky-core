use std::fmt;
use std::str::FromStr;

/// The key to limit the quota on.
#[derive(Debug, Clone, PartialEq, Eq)]
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