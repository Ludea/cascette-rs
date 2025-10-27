//! TACT key management functionality
//!
//! This module provides centralized management of TACT encryption keys
//! used by the CASC system for content decryption.

pub mod commands;
pub mod manager;

pub use commands::TactCommands;
