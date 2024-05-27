//! Monotonic unix timestamp in microseconds

use std::time::SystemTime;
use std::{
    ops::{Add, Sub},
    sync::Mutex,
};

use once_cell::sync::Lazy;
use rand::Rng;

use crate::Error;

static NODE_ID: Lazy<u8> = Lazy::new(|| {
    let mut rng = rand::thread_rng();
    rng.gen::<u8>()
});
static COUNTER: Lazy<Mutex<u8>> = Lazy::new(|| Mutex::new(0));

/// Monotonic timestamp since [SystemTime::UNIX_EPOCH] in microseconds as u64
///
/// Uses 8 bits machine ID
/// Encoded and decoded as LE bytes.
///
/// Valid for the next 500 thousand years!
#[derive(Debug, PartialEq, PartialOrd)]
pub struct Timestamp(pub(crate) u64);

impl Timestamp {
    pub fn now() -> Self {
        let mut counter = COUNTER.lock().unwrap();
        *counter = (*counter).wrapping_add(1);

        let time =
            (system_time() & 0xFFFFFFFFFFFF0000) | ((*NODE_ID as u64) << 8) | (*counter as u64);

        Self(time)
    }

    pub fn to_bytes(&self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    pub fn difference(&self, rhs: &Timestamp) -> u64 {
        self.0.abs_diff(rhs.0)
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

impl From<[u8; 8]> for Timestamp {
    fn from(bytes: [u8; 8]) -> Self {
        Self(u64::from_le_bytes(bytes))
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
        let mut set = HashSet::new();

        const COUNT: usize = 100;

        for _ in 0..COUNT {
            set.insert(Timestamp::now().0);
        }

        assert_eq!(set.len(), COUNT)
    }
}
