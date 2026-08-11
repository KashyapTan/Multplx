//! Typed runtime backend facade, supported transports, and dispatch helpers.

pub mod actor_state;
pub mod cmux;
pub mod command;
pub mod facade;
pub mod harness;
pub mod harness_launch;
pub mod headroom;
pub mod herdr;
pub mod herdr_cleanup;
pub mod herdr_presentation;
pub mod herdr_tools;
pub mod herdr_wire;
pub mod tmux;
pub mod treehouse_tools;

/// Identifies the current implementation boundary in diagnostics and tests.
pub const SHADOW_BOUNDARY: &str = "backend-dispatch-rust";
