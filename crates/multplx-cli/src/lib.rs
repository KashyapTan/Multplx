//! Command-line dispatch for the shadow Rust runtime.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use clap::{Parser, Subcommand};

/// The Portion 01 command-line surface.
#[derive(Debug, Parser)]
#[command(
    name = "mx",
    version,
    about = "Multplx Rust shadow runtime (no production commands enabled)",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Verify that the release-mode shadow binary and crate graph are available.
    #[command(hide = true)]
    ShadowDiagnostic,
}

impl Cli {
    /// Parses an explicit `mx <subcommand>` invocation or an `mx-<subcommand>`
    /// compatibility entry point.
    pub fn parse_multicall<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        if let Some(program) = args.first().cloned()
            && let Some(alias) = multicall_alias(&program)
        {
            args.insert(1, alias);
        }
        Self::parse_from(args)
    }

    /// Runs the selected shadow-only command.
    pub fn run(self) {
        match self.command {
            Command::ShadowDiagnostic => {
                let boundaries = [
                    multplx_core::SHADOW_BOUNDARY,
                    multplx_domain::SHADOW_BOUNDARY,
                    multplx_backend::SHADOW_BOUNDARY,
                    multplx_services::SHADOW_BOUNDARY,
                ];
                debug_assert_eq!(boundaries.len(), 4);
                println!("multplx rust shadow: ready");
            }
        }
    }
}

fn multicall_alias(program: &OsStr) -> Option<OsString> {
    let file_name = Path::new(program).file_name()?.to_str()?;
    file_name.strip_prefix("mx-").map(OsString::from)
}

#[cfg(test)]
mod tests {
    use super::multicall_alias;
    use std::ffi::{OsStr, OsString};

    #[test]
    fn extracts_multicall_alias_from_executable_name() {
        assert_eq!(
            multicall_alias(OsStr::new("/tmp/mx-shadow-diagnostic")),
            Some(OsString::from("shadow-diagnostic"))
        );
    }

    #[test]
    fn leaves_canonical_binary_without_an_alias() {
        assert_eq!(multicall_alias(OsStr::new("/tmp/mx")), None);
    }
}
