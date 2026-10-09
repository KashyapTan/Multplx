//! Local syntax preflight, without claiming provider/account model availability.
use std::ffi::OsString;

pub fn validate(harness: &str, model: &str) -> Result<(), String> {
    if model.is_empty() || model.chars().any(char::is_whitespace) {
        let guidance = match harness {
            "codex" => {
                "use a canonical model ID such as 'gpt-6-luna' rather than the display label 'GPT-6 Luna'"
            }
            "claude" => "use a provider model ID or an installed Claude model alias",
            "cursor" => "use a model ID reported by 'agent models'",
            "pi" => "use the provider model ID reported by Pi's model discovery",
            _ => "use the provider's canonical model ID",
        };
        return Err(format!(
            "invalid {harness} model selection {model:?}: {guidance}; custom single-token IDs are preserved, and local validation does not establish account access"
        ));
    }
    Ok(())
}

pub fn validate_cli(harness: &str, args: &[OsString]) -> Result<(), String> {
    for (index, argument) in args.iter().enumerate() {
        let Some(argument) = argument.to_str() else {
            continue;
        };
        let model = if matches!(argument, "--model" | "-m") {
            args.get(index + 1).and_then(|value| value.to_str())
        } else {
            argument.strip_prefix("--model=").or_else(|| {
                argument
                    .strip_prefix("-m")
                    .filter(|value| !value.is_empty())
            })
        };
        if let Some(model) = model {
            validate(harness, model)?;
        }
        if harness == "codex" {
            let config = if matches!(argument, "-c" | "--config") {
                args.get(index + 1).and_then(|value| value.to_str())
            } else {
                argument.strip_prefix("--config=").or_else(|| {
                    argument
                        .strip_prefix("-c")
                        .filter(|value| !value.is_empty())
                })
            };
            if let Some((key, value)) = config.and_then(|config| config.split_once('='))
                && key.trim().trim_matches(['\'', '"']) == "model"
            {
                let value = value.trim();
                let decoded = serde_json::from_str::<String>(value)
                    .unwrap_or_else(|_| value.trim_matches('\'').to_owned());
                validate(harness, &decoded)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_labels_fail_without_restricting_custom_ids() {
        for harness in ["codex", "claude", "cursor", "pi"] {
            assert!(validate(harness, "GPT-6 Luna").is_err());
            for model in [
                "default",
                "gpt-6-luna",
                "custom/provider-model:latest",
                "vendor-new-model[effort=high]",
            ] {
                validate(harness, model).unwrap();
            }
        }
        assert!(
            validate("codex", "GPT-6 Luna")
                .unwrap_err()
                .contains("gpt-6-luna")
        );
        assert!(validate_cli("codex", &["--model=GPT-6 Luna".into()]).is_err());
    }
}
