//! Supervisor endpoint discovery from `bin/mx-supervisor-target-lib.sh`.

/// Default target outside a detected backend.
pub const DEFAULT_TARGET: &str = "broker:0";
/// Default backend outside a detected backend.
pub const DEFAULT_BACKEND: &str = "tmux";

/// Environment inputs to deterministic supervisor discovery.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SupervisorEnvironment {
    /// Explicit target override.
    pub target: Option<String>,
    /// Explicit backend override.
    pub backend: Option<String>,
    /// Current tmux pane marker.
    pub tmux_pane: Option<String>,
    /// Whether Herdr marks this process as managed.
    pub herdr_environment: bool,
    /// Current Herdr pane identifier.
    pub herdr_pane_id: Option<String>,
    /// Current Herdr session or `default`.
    pub herdr_session: Option<String>,
}

/// One discovered value plus whether it came from positive evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Discovery {
    /// Target or backend text.
    pub value: String,
    /// False only for the legacy fallback.
    pub detected: bool,
}

/// Resolve target with explicit, tmux, Herdr, fallback precedence.
#[must_use]
pub fn target(environment: &SupervisorEnvironment) -> Discovery {
    if let Some(value) = environment
        .target
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        return Discovery {
            value: value.clone(),
            detected: true,
        };
    }
    if let Some(value) = environment
        .tmux_pane
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        return Discovery {
            value: value.clone(),
            detected: true,
        };
    }
    if environment.herdr_environment
        && let Some(pane) = environment
            .herdr_pane_id
            .as_ref()
            .filter(|value| !value.is_empty())
    {
        return Discovery {
            value: format!(
                "{}:{pane}",
                environment.herdr_session.as_deref().unwrap_or("default")
            ),
            detected: true,
        };
    }
    Discovery {
        value: DEFAULT_TARGET.to_owned(),
        detected: false,
    }
}

/// Resolve backend independently with the same precedence.
#[must_use]
pub fn backend(environment: &SupervisorEnvironment) -> Discovery {
    if let Some(value) = environment
        .backend
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        return Discovery {
            value: value.clone(),
            detected: true,
        };
    }
    if environment
        .tmux_pane
        .as_ref()
        .is_some_and(|value| !value.is_empty())
    {
        return Discovery {
            value: "tmux".to_owned(),
            detected: true,
        };
    }
    if environment.herdr_environment
        && environment
            .herdr_pane_id
            .as_ref()
            .is_some_and(|value| !value.is_empty())
    {
        return Discovery {
            value: "herdr".to_owned(),
            detected: true,
        };
    }
    Discovery {
        value: DEFAULT_BACKEND.to_owned(),
        detected: false,
    }
}

#[cfg(test)]
mod tests {
    use super::{SupervisorEnvironment, backend, target};

    #[test]
    fn nested_tmux_wins_over_herdr() {
        let environment = SupervisorEnvironment {
            tmux_pane: Some("%7".to_owned()),
            herdr_environment: true,
            herdr_pane_id: Some("pane-1".to_owned()),
            herdr_session: Some("lab".to_owned()),
            ..SupervisorEnvironment::default()
        };
        assert_eq!(target(&environment).value, "%7");
        assert_eq!(backend(&environment).value, "tmux");
    }
}
