//! Fail-closed deep-review lifecycle refusal from `bin/mx-gate-refuse-lib.sh`.

/// The established deep-review refusal exit code.
pub const REFUSAL_EXIT: u8 = 3;

/// Exact refusal diagnostic.
pub const REFUSAL_MESSAGE: &str =
    "error: deep-review agent must not drive Multplx lifecycle (DEEP_REVIEW_GATE set)";

/// Return whether lifecycle work must be refused for the supplied environment.
#[must_use]
pub fn is_gate_agent(gate_is_set: bool, test_bypass: bool) -> bool {
    gate_is_set && !test_bypass
}
