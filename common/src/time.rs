//! Monotonic unix timestamp in microseconds

use std::fmt::Display;
use std::time::SystemTime;
use std::{
    ops::{Add, Sub},
    sync::Mutex,
};

use once_cell::sync::Lazy;
use rand::Rng;

use crate::{Error, Result};

/// 1 in 1024 chance of collision with another machine.
const DEFAULT_CLOCK_BITS: u8 = 10;

pub struct TimestampFactory {
    clock_mask: u64,
    time_mask: u64,
    clock_id: u64,
    last_time: u64,
}

impl TimestampFactory {
    pub fn new(clock_bits: u8) -> Self {
        let clock_mask = (1 << clock_bits) - 1;
        let time_mask = !0 >> clock_bits;

        Self {
            clock_mask,
            time_mask,
            clock_id: rand::thread_rng().gen::<u64>() & clock_mask,
            last_time: system_time() & time_mask,
        }
    }

    pub fn now(&mut self) -> Timestamp {
        // Ensure the system time stays monotonic (doesn't move to past values).
        self.last_time = (system_time() & self.time_mask).max(self.last_time + self.clock_mask + 1);

        // Add clock_id to the end of the timestamp
        Timestamp(self.last_time | self.clock_id)
    }
}

impl Default for TimestampFactory {
    fn default() -> Self {
        Self::new(DEFAULT_CLOCK_BITS)
    }
}

static DEFAULT_FACTORY: Lazy<Mutex<TimestampFactory>> =
    Lazy::new(|| Mutex::new(TimestampFactory::default()));

/// Monotonic timestamp since [SystemTime::UNIX_EPOCH] in microseconds as u64
///
/// Uses 10 bits of randomness for clock id
/// Encoded and decoded as BE bytes (for order preserving).
/// Stringified as BE bytes encoded with [base32::Alphabet::Crockford]
///
/// Valid for the next 500 thousand years!
#[derive(Debug, Clone, PartialEq, PartialOrd, Hash, Eq, Ord)]
pub struct Timestamp(u64);

impl Timestamp {
    pub fn now() -> Self {
        DEFAULT_FACTORY.lock().unwrap().now()
    }

    pub fn to_bytes(&self) -> [u8; 8] {
        self.0.to_be_bytes()
    }

    pub fn difference(&self, rhs: &Timestamp) -> u64 {
        self.0.abs_diff(rhs.0)
    }
}

impl Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes: [u8; 8] = self.into();
        f.write_str(&base32::encode(base32::Alphabet::Crockford, &bytes))
    }
}

impl TryFrom<String> for Timestamp {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        match base32::decode(base32::Alphabet::Crockford, &value) {
            Some(vec) => {
                let bytes: [u8; 8] = vec.try_into().map_err(|_| {
                    Error::Generic("Could not decode timestamp from string".to_string())
                })?;

                Ok(bytes.into())
            }
            None => Err(Error::Generic(
                "Could not decode timestamp from string".to_string(),
            )),
        }
    }
}

impl TryFrom<&[u8]> for Timestamp {
    type Error = Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let bytes: [u8; 8] = bytes
            .try_into()
            .map_err(|_| Error::Generic("Timestamp should be 8 bytes".to_string()))?;

        Ok(bytes.into())
    }
}

impl From<&Timestamp> for [u8; 8] {
    fn from(timestamp: &Timestamp) -> Self {
        timestamp.0.to_be_bytes()
    }
}

impl From<[u8; 8]> for Timestamp {
    fn from(bytes: [u8; 8]) -> Self {
        Self(u64::from_be_bytes(bytes))
    }
}

impl Add<u64> for Timestamp {
    type Output = Timestamp;

    fn add(self, rhs: u64) -> Self::Output {
        Timestamp(self.0 + rhs)
    }
}

impl Sub<u64> for Timestamp {
    type Output = Timestamp;

    fn sub(self, rhs: u64) -> Self::Output {
        Timestamp(self.0 - rhs)
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Return the number of microseconds since [SystemTime::UNIX_EPOCH]
fn system_time() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time drift")
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn monotonic() {
        const COUNT: usize = 100;

        let mut set = HashSet::with_capacity(COUNT);
        let mut vec = Vec::with_capacity(COUNT);

        for _ in 0..COUNT {
            let timestamp = Timestamp::now();

            set.insert(timestamp.clone());
            vec.push(timestamp);
        }

        let mut ordered = vec.clone();
        ordered.sort();

        assert_eq!(set.len(), COUNT, "unique");
        assert_eq!(ordered, vec, "ordered");
    }

    #[test]
    fn strings() {
        const COUNT: usize = 100;

        let mut set = HashSet::with_capacity(COUNT);
        let mut vec = Vec::with_capacity(COUNT);

        for _ in 0..COUNT {
            let string = Timestamp::now().to_string();

            set.insert(string.clone());
            vec.push(string)
        }

        dbg!(&vec);
        let mut ordered = vec.clone();
        ordered.sort();

        dbg!(&vec);
        assert_eq!(set.len(), COUNT, "unique");
        assert_eq!(ordered, vec, "ordered");
    }

    #[test]
    fn to_from_string() {
        let timestamp = Timestamp::now();
        let string = timestamp.to_string();
        let decoded: Timestamp = string.try_into().unwrap();

        assert_eq!(decoded, timestamp)
    }
}
