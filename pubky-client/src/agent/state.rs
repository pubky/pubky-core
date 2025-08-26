use std::marker::PhantomData;

use pkarr::Keypair;

/// Type-state markers
#[derive(Debug, Clone, Copy)]
pub struct Keyed;
#[derive(Debug, Clone, Copy)]
pub struct Keyless;

/// Aliases for ergonomics
pub type KeyedAgent = super::core::PubkyAgent<Keyed>;
pub type KeylessAgent = super::core::PubkyAgent<Keyless>;

/// Sealed to keep the state space closed
pub(crate) mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Keyed {}
    impl Sealed for super::Keyless {}
}

/// Helper wrapper for holding an optional keypair in type-state
#[derive(Debug, Clone)]
pub struct MaybeKeypair<S> {
    inner: Option<Keypair>,
    _marker: PhantomData<S>,
}

impl MaybeKeypair<Keyed> {
    pub fn new(kp: Keypair) -> Self {
        Self {
            inner: Some(kp),
            _marker: PhantomData,
        }
    }
    pub fn get(&self) -> &Keypair {
        self.inner.as_ref().expect("keyed agent always has keypair")
    }
}
impl MaybeKeypair<Keyless> {
    pub fn new_none() -> Self {
        Self {
            inner: None,
            _marker: PhantomData,
        }
    }
}
