//! Typed runtime backend facade and the reference tmux implementation.
//!
//! Portion 04 keeps this implementation shadow-only. Production backend
//! selection remains on the retained compatibility path until Herdr and cmux
//! implement the same safety contract in Portions 05 and 06.

pub mod actor_state;
pub mod command;
pub mod facade;
pub mod tmux;

/// Identifies the current implementation boundary in diagnostics and tests.
pub const SHADOW_BOUNDARY: &str = "backend-tmux-shadow";
