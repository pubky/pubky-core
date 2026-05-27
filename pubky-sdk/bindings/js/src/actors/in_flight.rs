use std::cell::RefCell;

use crate::js_error::{JsResult, PubkyError};

/// RAII guard for JS-exposed async methods that must not overlap on one actor.
///
/// wasm-bindgen methods can be called again from JavaScript while a previous
/// `async fn` is still suspended. Auth flows mutate or consume their inner Rust
/// state while awaiting relay responses, so overlapping calls would otherwise
/// race against the same `RefCell<Option<...>>` state and produce confusing
/// errors or panics.
///
/// The guard sets a shared in-flight flag when the call starts and clears it in
/// `Drop`, including when the async method returns early with an error.
pub(super) struct InFlightGuard<'a> {
    in_flight: &'a RefCell<bool>,
}

impl<'a> InFlightGuard<'a> {
    /// Try to enter an actor method's critical async section.
    ///
    /// Returns the caller-provided `in_use_error` when another JS-visible call
    /// is already in progress. The closure lets each actor keep its own
    /// user-facing error message while sharing the flag lifecycle.
    pub(super) fn begin(
        in_flight: &'a RefCell<bool>,
        in_use_error: impl FnOnce() -> PubkyError,
    ) -> JsResult<Self> {
        let mut flag = in_flight.borrow_mut();
        if *flag {
            Err(in_use_error())
        } else {
            *flag = true;
            Ok(Self { in_flight })
        }
    }
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        let mut flag = self.in_flight.borrow_mut();
        *flag = false;
    }
}
