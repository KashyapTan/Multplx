//! Canonical construction and classification for the Multplx operational-input
//! protocol formerly owned by `bin/mx-operational-input.sh`.

use std::fmt;

/// Permanent compatibility prefix, including U+2063 INVISIBLE SEPARATOR.
pub const PREFIX: &str = "\u{2063}MULTPLX_OP: ";
/// Current wire version.
pub const VERSION: &str = "v1";
/// Live-charter-compatible from-broker carrier.
pub const FROM_BROKER_MARK: &str = "[mx-from-broker]\u{2063}";
/// Exact pre-protocol session-start prompt retained for transcript parsing.
pub const LEGACY_SESSION_START: &str =
    "Run `bin/mx-session-start.sh` now, exactly once, before executing any other instructions.";
/// Exact pre-protocol watcher prefix.
pub const LEGACY_WATCHER_PREFIX: &str = "MULTPLX WATCHER WAKE: ";
/// Exact pre-protocol watcher suffix.
pub const LEGACY_WATCHER_SUFFIX: &str = "\n\nRun bin/mx-wake-drain.sh first and handle the queued wake. Watcher continuity is extension-owned.";
/// Exact pre-protocol turn-end prefix.
pub const LEGACY_TURN_END_PREFIX: &str = "TURN WOULD END BLIND - supervision is off. The watcher cycle is missing, failed, or unhealthy. Follow the harness recovery instruction below before ending the turn.\n\n";
/// Exact pre-protocol away prefix.
pub const LEGACY_AWAY_PREFIX: &str = "\u{2063}Supervisor escalate (";

/// Closed current construction vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    SessionStart,
    Watcher,
    TurnEndGuard,
    AwaySupervisor,
    LaunchBrief,
    FromBroker,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OperationalInputCodec;

impl OperationalInputCodec {
    #[must_use]
    pub fn construct(self, kind: Kind, body: &str) -> Option<String> {
        construct(kind, body)
    }

    #[must_use]
    pub fn kind(self, message: &str) -> Option<Kind> {
        current_kind(message)
    }

    #[must_use]
    pub fn body(self, message: &str) -> Option<&str> {
        body(message)
    }

    #[must_use]
    pub fn classify(self, message: &str) -> Option<&'static str> {
        classify(message)
    }
}

impl Kind {
    /// Parse a current producer kind.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "session-start" => Some(Self::SessionStart),
            "watcher" => Some(Self::Watcher),
            "turn-end-guard" => Some(Self::TurnEndGuard),
            "away-supervisor" => Some(Self::AwaySupervisor),
            "launch-brief" => Some(Self::LaunchBrief),
            "from-broker" => Some(Self::FromBroker),
            _ => None,
        }
    }

    /// Wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "session-start",
            Self::Watcher => "watcher",
            Self::TurnEndGuard => "turn-end-guard",
            Self::AwaySupervisor => "away-supervisor",
            Self::LaunchBrief => "launch-brief",
            Self::FromBroker => "from-broker",
        }
    }

    const fn is_generic(self) -> bool {
        !matches!(self, Self::FromBroker)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Construct one current operational input without interpreting its body.
pub fn construct(kind: Kind, body: &str) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    if kind == Kind::FromBroker {
        return Some(mark_from_broker(body));
    }
    Some(format!("{PREFIX}{VERSION} {}: {body}", kind.as_str()))
}

/// Add the established from-broker carrier idempotently.
#[must_use]
pub fn mark_from_broker(body: &str) -> String {
    if body.starts_with(FROM_BROKER_MARK) && body.len() > FROM_BROKER_MARK.len() {
        body.to_owned()
    } else {
        format!("{FROM_BROKER_MARK}{body}")
    }
}

/// Parse only current typed inputs.
#[must_use]
pub fn current_kind(message: &str) -> Option<Kind> {
    if message.starts_with(FROM_BROKER_MARK) && message.len() > FROM_BROKER_MARK.len() {
        return Some(Kind::FromBroker);
    }
    let remainder = message.strip_prefix(&format!("{PREFIX}{VERSION} "))?;
    let (raw_kind, body) = remainder.split_once(": ")?;
    let kind = Kind::parse(raw_kind)?;
    if !kind.is_generic() || body.is_empty() {
        return None;
    }
    Some(kind)
}

/// Recover the exact body of a current input.
#[must_use]
pub fn body(message: &str) -> Option<&str> {
    if message.starts_with(FROM_BROKER_MARK) && message.len() > FROM_BROKER_MARK.len() {
        return message.strip_prefix(FROM_BROKER_MARK);
    }
    let kind = current_kind(message)?;
    message.strip_prefix(&format!("{PREFIX}{VERSION} {}: ", kind.as_str()))
}

/// Classify a historical pre-protocol transcript input.
#[must_use]
pub fn legacy_kind(message: &str) -> Option<&'static str> {
    if message.starts_with(PREFIX) && message.len() > PREFIX.len() {
        return Some("legacy-operational");
    }
    if message == LEGACY_SESSION_START {
        return Some("session-start");
    }
    if message.starts_with(LEGACY_AWAY_PREFIX) {
        return Some("away-supervisor");
    }
    if message.starts_with(LEGACY_WATCHER_PREFIX)
        && message.ends_with(LEGACY_WATCHER_SUFFIX)
        && message.len() > LEGACY_WATCHER_PREFIX.len() + LEGACY_WATCHER_SUFFIX.len()
    {
        return Some("watcher");
    }
    if message.starts_with(LEGACY_TURN_END_PREFIX) && message.len() > LEGACY_TURN_END_PREFIX.len() {
        return Some("turn-end-guard");
    }
    None
}

/// Classify current input first and historical input second.
#[must_use]
pub fn classify(message: &str) -> Option<&'static str> {
    current_kind(message)
        .map(Kind::as_str)
        .or_else(|| legacy_kind(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_kinds_round_trip_literal_multiline_bodies() {
        for kind in [
            Kind::SessionStart,
            Kind::Watcher,
            Kind::TurnEndGuard,
            Kind::AwaySupervisor,
            Kind::LaunchBrief,
            Kind::FromBroker,
        ] {
            let payload = "line one\nline two\n";
            let encoded = construct(kind, payload).expect("encoded");
            assert_eq!(current_kind(&encoded), Some(kind));
            assert_eq!(body(&encoded), Some(payload));
            assert_eq!(classify(&encoded), Some(kind.as_str()));
        }
    }

    #[test]
    fn malformed_and_near_miss_inputs_remain_unclassified() {
        for value in [
            "MULTPLX_OP: v1 watcher: body",
            "\u{2063} arbitrary maintainer text",
            "[mx-from-broker] inspect this label",
        ] {
            assert_eq!(classify(value), None, "{value:?}");
        }
        for value in [
            "\u{2063}MULTPLX_OP: v2 watcher: body",
            "\u{2063}MULTPLX_OP: v1 unknown: body",
            "\u{2063}MULTPLX_OP: v1 watcher: ",
        ] {
            assert_eq!(current_kind(value), None, "{value:?}");
            assert_eq!(classify(value), Some("legacy-operational"), "{value:?}");
        }
        assert!(construct(Kind::Watcher, "").is_none());
    }

    #[test]
    fn legacy_shapes_are_isolated_from_current_parser() {
        let watcher = format!("{LEGACY_WATCHER_PREFIX}signal{LEGACY_WATCHER_SUFFIX}");
        let cases = [
            (LEGACY_SESSION_START, "session-start"),
            (&watcher, "watcher"),
            (
                "TURN WOULD END BLIND - supervision is off. The watcher cycle is missing, failed, or unhealthy. Follow the harness recovery instruction below before ending the turn.\n\nfailed",
                "turn-end-guard",
            ),
            ("\u{2063}Supervisor escalate (one)", "away-supervisor"),
            ("\u{2063}MULTPLX_OP: old", "legacy-operational"),
        ];
        for (message, expected) in cases {
            assert_eq!(current_kind(message), None);
            assert_eq!(legacy_kind(message), Some(expected));
        }
    }

    #[test]
    fn typed_codec_and_kind_parsing_cover_all_public_paths() {
        let codec = OperationalInputCodec;
        for (text, kind) in [
            ("session-start", Kind::SessionStart),
            ("watcher", Kind::Watcher),
            ("turn-end-guard", Kind::TurnEndGuard),
            ("away-supervisor", Kind::AwaySupervisor),
            ("from-broker", Kind::FromBroker),
            ("launch-brief", Kind::LaunchBrief),
        ] {
            assert_eq!(Kind::parse(text), Some(kind));
            assert_eq!(kind.to_string(), text);
            let encoded = codec.construct(kind, "payload").expect("construct");
            assert_eq!(codec.kind(&encoded), Some(kind));
            assert_eq!(codec.body(&encoded), Some("payload"));
            assert_eq!(codec.classify(&encoded), Some(text));
        }
        assert_eq!(Kind::parse("legacy-operational"), None);
        assert_eq!(codec.construct(Kind::Watcher, ""), None);
        assert_eq!(codec.kind("plain"), None);
        assert_eq!(codec.body("plain"), None);
        assert_eq!(codec.classify("plain"), None);
        assert_eq!(mark_from_broker("body"), format!("{FROM_BROKER_MARK}body"));
    }
}
