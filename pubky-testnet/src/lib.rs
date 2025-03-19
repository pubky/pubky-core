// Actual testnet exposed in the library
mod testnet;
pub use testnet::Testnet;

// E2E tests
#[cfg(test)]
mod e2e_tests;
