use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

/// Burst size
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Burst(pub NonZeroU32);

impl fmt::Display for Burst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}burst", self.0)
    }
}

impl FromStr for Burst {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.ends_with("burst") {
            let num_str = &s[0..s.len() - 5]; // Remove "burst"
            match num_str.parse::<NonZeroU32>() {
                Ok(number) => Ok(Burst(number)),
                Err(_) => Err(format!("Failed to parse burst value from '{}'", s)),
            }
        } else {
            Err(format!("Invalid burst format: '{}', expected {{number}}burst", s))
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_burst_from_str() {
        let burst = Burst::from_str("10burst").unwrap();
        assert_eq!(burst, Burst(NonZeroU32::new(10).unwrap()));
    }

    #[test]
    fn test_burst_from_str_invalid() {
        let burst = Burst::from_str("10");
        assert!(burst.is_err());
    }

    #[test]
    fn test_burst_display() {
        let burst = Burst(NonZeroU32::new(10).unwrap());
        assert_eq!(burst.to_string(), "10burst");
    }
    
}