//! Generic structured one-shot agent transport construction.
//!
//! This module owns only the process and file arguments shared by ordinary
//! workflow stages and explicit review.  Policy, time bounds, result schemas,
//! task identity and completion checks remain with each caller.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

pub(crate) fn structured_command(
    program: impl AsRef<OsStr>,
    schema: &Path,
    prompt: &Path,
    output: &Path,
    session: &Path,
) -> Command {
    let mut command = Command::new(program);
    command
        .args(["--session", "new", "--schema"])
        .arg(schema)
        .arg("--prompt")
        .arg(prompt)
        .arg("--output")
        .arg(output)
        .arg("--session-out")
        .arg(session);
    command
}
