//! Local service boundaries for the incremental Multplx Rust port.
//!
//! Portion 08 owns the task-bound status-reporting MCP transport.

pub mod report_mcp;

/// Identifies the current implementation boundary in diagnostics and tests.
pub const SHADOW_BOUNDARY: &str = "services-report-mcp";
