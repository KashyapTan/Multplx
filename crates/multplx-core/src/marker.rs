//! Live-charter-compatible from-broker routing carrier.
//!
//! `bin/mx-marker-lib.sh` is a compatibility adapter to the full operational
//! input protocol. Portion 02 owns only the established marked-routing bytes;
//! Portion 03 retains ownership of the broader protocol parser.

/// Terminal-safe invisible separator U+2063.
pub const OPERATIONAL_MARK: &str = "\u{2063}";
/// Established from-broker label.
pub const FROM_BROKER_LABEL: &str = "[mx-from-broker]";

/// Return the complete compatibility marker.
#[must_use]
pub fn from_broker_marker() -> String {
    format!("{FROM_BROKER_LABEL}{OPERATIONAL_MARK}")
}

/// Return whether a message carries the exact established marker and a body.
#[must_use]
pub fn is_from_broker(message: &str) -> bool {
    message
        .strip_prefix(&from_broker_marker())
        .is_some_and(|body| !body.is_empty())
}

/// Prefix an unmarked message exactly once.
#[must_use]
pub fn mark_from_broker(message: &str) -> String {
    if is_from_broker(message) {
        message.to_owned()
    } else {
        format!("{}{message}", from_broker_marker())
    }
}

#[cfg(test)]
mod tests {
    use super::{from_broker_marker, is_from_broker, mark_from_broker};

    #[test]
    fn marker_is_byte_compatible_and_idempotent() {
        assert_eq!(
            from_broker_marker().as_bytes(),
            b"[mx-from-broker]\xe2\x81\xa3"
        );
        let marked = mark_from_broker("do work");
        assert!(is_from_broker(&marked));
        assert_eq!(mark_from_broker(&marked), marked);
        assert!(!is_from_broker("[mx-from-broker]do work"));
    }
}
