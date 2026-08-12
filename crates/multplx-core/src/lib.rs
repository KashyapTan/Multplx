//! Typed, durable primitives shared by the incremental Multplx Rust port.
//!
//! Portion 02 transfers the reusable contracts from the primitive shell
//! libraries into this crate. Production shell callers remain authoritative
//! until their owning portions cut over, while the APIs here are exercised by
//! Rust-native and differential compatibility tests.

pub mod backend_hometag;
pub mod checks;
pub mod classification;
pub mod command_policy;
pub mod composer;
pub mod error;
pub mod filesystem;
pub mod gate_refuse;
pub mod identifiers;
pub mod journal;
pub mod locks;
pub mod marker;
pub mod paths;
pub mod primary_scope;
pub mod probe;
pub mod process;
pub mod session_lock;
pub mod supervision;
pub mod supervisor_target;
pub mod tangle;
pub mod tmux;
pub mod transition;
pub mod wake;

#[cfg(test)]
mod coverage_tests;

/// Identifies the current implementation boundary in diagnostics and tests.
pub const SHADOW_BOUNDARY: &str = "core-primitives-shadow";
