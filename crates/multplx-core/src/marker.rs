//! Live-charter-compatible from-parent routing carrier.
//!
//! `bin/mx-marker-lib.sh` is a compatibility adapter to the full operational
//! input protocol. Portion 02 owns only the established marked-routing bytes;
//! Portion 03 retains ownership of the broader protocol parser.

/// Terminal-safe invisible separator U+2063.
pub const OPERATIONAL_MARK: &str = "\u{2063}";
/// Established from-parent label.
pub const FROM_PARENT_LABEL: &str = "[mx-from-parent]";

/// Return the complete compatibility marker.
#[must_use]
pub fn from_parent_marker() -> String {
    format!("{FROM_PARENT_LABEL}{OPERATIONAL_MARK}")
}

/// Return whether a message carries the exact established marker and a body.
#[must_use]
pub fn is_from_parent(message: &str) -> bool {
    [
        from_parent_marker(),
        format!("[mx-from-broker]{OPERATIONAL_MARK}"),
    ]
    .iter()
    .any(|marker| {
        message
            .strip_prefix(marker)
            .is_some_and(|body| !body.is_empty())
    })
}

/// Prefix an unmarked message exactly once.
#[must_use]
pub fn mark_from_parent(message: &str) -> String {
    let body = message
        .strip_prefix(&from_parent_marker())
        .or_else(|| message.strip_prefix(&format!("[mx-from-broker]{OPERATIONAL_MARK}")))
        .unwrap_or(message);
    format!("{}{body}", from_parent_marker())
}

#[cfg(test)]
mod tests {
    use super::{from_parent_marker, is_from_parent, mark_from_parent};

    #[test]
    fn marker_is_byte_compatible_and_idempotent() {
        assert_eq!(
            from_parent_marker().as_bytes(),
            b"[mx-from-parent]\xe2\x81\xa3"
        );
        let marked = mark_from_parent("do work");
        assert!(is_from_parent(&marked));
        assert_eq!(mark_from_parent(&marked), marked);
        assert!(!is_from_parent("[mx-from-parent]do work"));
        assert!(!is_from_parent("[mx-from-broker]do work"));
        let legacy = "[mx-from-broker]\u{2063}do work";
        assert!(is_from_parent(legacy));
        assert_eq!(mark_from_parent(legacy), marked);
    }
}
