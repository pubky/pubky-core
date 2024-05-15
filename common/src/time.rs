//! Simple handling for timestamps

use std::{ops::Add, time::SystemTime};

/// Timestamp since [SystemTime::UNIX_EPOCH] in microseconds as u64
#[derive(Debug, PartialEq, PartialOrd)]
pub struct Timestamp(pub(crate) u64);

impl Timestamp {
    pub fn now() -> Self {
        Self(system_time())
    }

    /// Encode Timestamp as Big-Endian 8 bytes
    pub fn encode(&self, bytes: &mut [u8]) {
        bytes.copy_from_slice(&self.0.to_be_bytes())
    }
}

impl Add<u64> for Timestamp {
    type Output = Timestamp;

    fn add(self, rhs: u64) -> Self::Output {
        Timestamp(self.0 + rhs)
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
