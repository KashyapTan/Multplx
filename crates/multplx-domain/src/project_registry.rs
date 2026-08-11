//! Read-only project delivery-mode resolution from `data/projects.md`.

use std::fs;
use std::path::Path;

/// Supported delivery posture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryMode {
    DeepReview,
    DirectPr,
    LocalOnly,
}

impl DeliveryMode {
    /// Registry spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeepReview => "deep-review",
            Self::DirectPr => "direct-PR",
            Self::LocalOnly => "local-only",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "deep-review" => Some(Self::DeepReview),
            "direct-PR" => Some(Self::DirectPr),
            "local-only" => Some(Self::LocalOnly),
            _ => None,
        }
    }
}

/// Resolved mode and optional warning, kept separate for exact stream routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resolution {
    pub mode: DeliveryMode,
    pub yolo: bool,
    pub warning: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProjectRegistry {
    path: std::path::PathBuf,
}

impl ProjectRegistry {
    #[must_use]
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn resolve(&self, name: &str) -> Resolution {
        resolve(&self.path, name)
    }
}

impl Resolution {
    /// Exact two-word stdout contract.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{} {}\n",
            self.mode.as_str(),
            if self.yolo { "on" } else { "off" }
        )
    }
}

fn fallback(warning: String) -> Resolution {
    Resolution {
        mode: DeliveryMode::DeepReview,
        yolo: false,
        warning: Some(warning),
    }
}

/// Resolve one exact project name without mutating or repairing the registry.
pub fn resolve(path: &Path, name: &str) -> Resolution {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => {
            return fallback(format!(
                "warn: no registry at {}; defaulting {name} to deep-review off",
                path.display()
            ));
        }
    };
    let mut parsed = None;
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() != Some(&"-") || fields.get(1) != Some(&name) {
            continue;
        }
        let mut mode = "deep-review".to_owned();
        let mut yolo = false;
        if fields.get(2).is_some_and(|field| field.starts_with('[')) {
            let mut bracket = Vec::new();
            for field in fields.iter().skip(2) {
                bracket.push(*field);
                if field.ends_with(']') {
                    break;
                }
            }
            let joined = bracket.join(" ");
            let inner = joined.trim_start_matches('[').trim_end_matches(']');
            let tokens: Vec<&str> = inner.split_whitespace().collect();
            if let Some(first) = tokens.first().copied()
                && first != "+yolo"
                && !first.is_empty()
            {
                mode = first.to_owned();
            }
            yolo = tokens.contains(&"+yolo");
        }
        parsed = Some((mode, yolo));
        break;
    }
    let Some((raw_mode, yolo)) = parsed else {
        return fallback(format!(
            "warn: project \"{name}\" not in registry; defaulting to deep-review off"
        ));
    };
    let Some(mode) = DeliveryMode::parse(&raw_mode) else {
        return fallback(format!(
            "warn: unknown mode \"{raw_mode}\" for {name}; defaulting to deep-review off"
        ));
    };
    Resolution {
        mode,
        yolo,
        warning: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_yolo_defaults_and_ambiguous_names_match_registry_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("projects.md");
        fs::write(
            &registry,
            "- app [local-only +yolo] - app\n- app-extra [direct-PR] - other\n- default - default\n- bad [unsafe +yolo] - bad\n",
        )
        .expect("registry");
        assert_eq!(resolve(&registry, "app").render(), "local-only on\n");
        assert_eq!(resolve(&registry, "app-extra").render(), "direct-PR off\n");
        assert_eq!(resolve(&registry, "default").render(), "deep-review off\n");
        let bad = resolve(&registry, "bad");
        assert_eq!(bad.render(), "deep-review off\n");
        assert!(bad.warning.expect("warning").contains("unknown mode"));
        assert!(resolve(&registry, "missing").warning.is_some());
        assert!(
            resolve(&temp.path().join("absent"), "app")
                .warning
                .is_some()
        );
    }

    #[test]
    fn typed_registry_and_delivery_mode_render_every_variant() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = temp.path().join("projects.md");
        fs::write(
            &registry,
            "- review [deep-review] - review\n- direct [direct-PR +yolo] - direct\n- local [local-only] - local\n",
        )
        .expect("registry");
        let typed = ProjectRegistry::new(&registry);
        assert_eq!(typed.resolve("review").mode, DeliveryMode::DeepReview);
        assert_eq!(typed.resolve("direct").mode, DeliveryMode::DirectPr);
        assert!(typed.resolve("direct").yolo);
        assert_eq!(typed.resolve("local").mode, DeliveryMode::LocalOnly);
        assert_eq!(typed.resolve("local").render(), "local-only off\n");
        assert_eq!(DeliveryMode::DeepReview.as_str(), "deep-review");
        assert_eq!(DeliveryMode::DirectPr.as_str(), "direct-PR");
        assert_eq!(DeliveryMode::LocalOnly.as_str(), "local-only");
    }
}
