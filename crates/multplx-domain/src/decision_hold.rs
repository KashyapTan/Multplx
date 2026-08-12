//! Durable maintainer-decision identities and single-resolution transitions.
//!
//! Backlog parsing and publication remain with [`crate::backlog`].  This module
//! owns the decision-specific identity, inventory, and retry invariants so a
//! resolved answer cannot be reopened or silently changed.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DecisionKey(String);

impl DecisionKey {
    pub fn parse(value: impl Into<String>) -> Result<Self, DecisionError> {
        let value = value.into();
        if value.is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(DecisionError(
                "decision key must be a privacy-safe slug".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HoldIdentity {
    origin: DecisionKey,
    key: DecisionKey,
}

impl HoldIdentity {
    pub fn parse(origin: impl Into<String>, key: impl Into<String>) -> Result<Self, DecisionError> {
        Ok(Self {
            origin: DecisionKey::parse(origin)?,
            key: DecisionKey::parse(key)?,
        })
    }

    #[must_use]
    pub fn id(&self) -> String {
        format!("{}-decision-{}", self.origin.as_str(), self.key.as_str())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DecisionInventory(BTreeSet<DecisionKey>);

impl DecisionInventory {
    pub fn parse_csv(value: &str) -> Result<Self, DecisionError> {
        let keys = value
            .split(',')
            .filter(|entry| !entry.is_empty())
            .map(|entry| DecisionKey::parse(entry.to_owned()))
            .collect::<Result<BTreeSet<_>, _>>()?;
        Ok(Self(keys))
    }

    pub fn union<'a>(
        &mut self,
        keys: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), DecisionError> {
        for key in keys {
            self.0.insert(DecisionKey::parse(key.to_owned())?);
        }
        Ok(())
    }

    pub fn verify_open<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), DecisionError> {
        for key in keys {
            let key = DecisionKey::parse(key.to_owned())?;
            if !self.0.contains(&key) {
                return Err(DecisionError(format!(
                    "open structured decision {} is outside the reviewed inventory",
                    key.as_str()
                )));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn render_csv(&self) -> String {
        self.0
            .iter()
            .map(DecisionKey::as_str)
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolutionIdentity {
    pub decision_digest: String,
    pub routed_to: Vec<String>,
}

impl ResolutionIdentity {
    pub fn new(
        decision: &[u8],
        routed_to: impl IntoIterator<Item = String>,
    ) -> Result<Self, DecisionError> {
        if decision.is_empty() {
            return Err(DecisionError("decision must not be empty".to_owned()));
        }
        if decision.len() > 8192 {
            return Err(DecisionError("decision exceeds 8192 bytes".to_owned()));
        }
        let mut routed_to = routed_to.into_iter().collect::<Vec<_>>();
        for task in &routed_to {
            DecisionKey::parse(task.clone())?;
        }
        routed_to.sort();
        routed_to.dedup();
        if routed_to.is_empty() {
            return Err(DecisionError(
                "at least one routed task is required".to_owned(),
            ));
        }
        Ok(Self {
            decision_digest: format!("{:x}", Sha256::digest(decision)),
            routed_to,
        })
    }

    pub fn accepts_retry(&self, candidate: &Self) -> Result<(), DecisionError> {
        if self == candidate {
            Ok(())
        } else {
            Err(DecisionError(
                "resolved hold retry changed the decision or routed identities".to_owned(),
            ))
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct DecisionError(String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_and_inventory_are_stable_and_sorted() {
        let identity = HoldIdentity::parse("review-1", "api-choice").expect("identity");
        assert_eq!(identity.id(), "review-1-decision-api-choice");
        let mut inventory = DecisionInventory::parse_csv("zeta,alpha").expect("inventory");
        inventory.union(["alpha", "middle"]).expect("union");
        assert_eq!(inventory.render_csv(), "alpha,middle,zeta");
        assert!(inventory.verify_open(["alpha", "zeta"]).is_ok());
        assert!(inventory.verify_open(["missing"]).is_err());
    }

    #[test]
    fn resolution_is_exact_idempotent_and_bounded() {
        let original = ResolutionIdentity::new(
            b"Use the smaller API.",
            ["task-b".to_owned(), "task-a".to_owned()],
        )
        .expect("resolution");
        let retry = ResolutionIdentity::new(
            b"Use the smaller API.",
            ["task-a".to_owned(), "task-b".to_owned()],
        )
        .expect("retry");
        assert!(original.accepts_retry(&retry).is_ok());
        let changed = ResolutionIdentity::new(
            b"Use the larger API.",
            ["task-a".to_owned(), "task-b".to_owned()],
        )
        .expect("changed");
        assert!(original.accepts_retry(&changed).is_err());
        assert!(ResolutionIdentity::new(&vec![b'x'; 8193], ["task-a".to_owned()]).is_err());
    }
}
