//! Typed local-state records and state machines for Multplx.
//!
//! Portion 03 moves backlog, project-registry, inherited configuration, and
//! operational-input behavior here while preserving every existing disk and
//! wire format.

pub mod backlog;
pub mod handoff;
pub mod inheritance;
pub mod lifecycle;
pub mod operational_input;
pub mod project_registry;

/// Identifies the current implementation boundary in diagnostics and tests.
pub const SHADOW_BOUNDARY: &str = "domain-local-state";
