//! TACT key orchestration and management
//!
//! This module provides high-level orchestration for TACT encryption keys,
//! managing key policies, assignments, and lifecycle while delegating the
//! actual cryptographic operations to cascette-crypto.

mod manager;

pub use manager::{TactKeyManager, TactKeyStats};
