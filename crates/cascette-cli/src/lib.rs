//! Library exports for cascette-cli
//! This file exposes modules for integration testing

pub mod installation;

// Re-export builds from installation for backward compatibility
pub use installation::builds;
