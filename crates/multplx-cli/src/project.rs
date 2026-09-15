//! Thin CLI for the canonical filesystem project owner.

use std::ffi::OsString;
use std::path::Path;

use multplx_domain::project_registry::{self, CheckoutOwnership};

const HELP: &str = "Usage: mx project <register|list|resolve|forget>\n\n  register PATH [--alias NAME] [--managed]\n      Remember an existing Git checkout; default ownership is user-owned.\n      --managed explicitly records a Multplx-owned clone for managed refresh.\n      Registration never changes checkout files or branches; repeated registration\n      cannot upgrade user-owned status. Linked worktrees retain distinct checkout IDs.\n  list\n      Print the version 2 project registry as JSON, including remembered missing paths.\n  resolve SELECTOR\n      Resolve an exact project/checkout ID, alias, name or canonical path.\n      Ambiguity lists candidate paths; output includes the current named base commit.\n  forget CHECKOUT_ID\n      Remove its registry location only; never delete checkout files.\n\nRegistration and resolution emit a task-ready immutable project/checkout/base binding.\nA changed display selection never updates existing task bindings. Local repositories\nneed no remote. Deep review is an explicit task selection, separate from publication.\nWorkspace discovery and the interactive selector are implemented in Phase 11.\n";

pub fn run(args: &[OsString]) -> i32 {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return 0;
    }
    let (_, home, _) = crate::active_paths();
    match execute(&home, args) {
        Ok(value) => {
            println!("{value}");
            0
        }
        Err(error) => {
            eprintln!("mx project: {error}");
            1
        }
    }
}

fn execute(home: &Path, args: &[OsString]) -> Result<String, String> {
    let action = args[0].to_str().ok_or("command must be UTF-8")?;
    match action {
        "list" if args.len() == 1 => {
            serde_json::to_string_pretty(&project_registry::read_catalog(home)?)
                .map_err(|e| e.to_string())
        }
        "resolve" if args.len() == 2 => {
            serde_json::to_string_pretty(&project_registry::resolve_checkout(
                home,
                args[1].to_str().ok_or("selector must be UTF-8")?,
            )?)
            .map_err(|e| e.to_string())
        }
        "forget" if args.len() == 2 => {
            project_registry::unregister_checkout(
                home,
                args[1].to_str().ok_or("checkout ID must be UTF-8")?,
            )?;
            Ok("{\"unregistered\":true}".to_owned())
        }
        "register" if args.len() >= 2 => {
            let mut alias = None;
            let mut ownership = CheckoutOwnership::UserOwned;
            let mut i = 2;
            while i < args.len() {
                match args[i].to_str() {
                    Some("--managed") if ownership == CheckoutOwnership::UserOwned => {
                        ownership = CheckoutOwnership::Managed
                    }
                    Some("--alias") if alias.is_none() => {
                        i += 1;
                        alias = Some(
                            args.get(i)
                                .and_then(|v| v.to_str())
                                .ok_or("--alias requires a UTF-8 name")?,
                        );
                    }
                    _ => return Err(format!("unexpected register argument {:?}", args[i])),
                }
                i += 1;
            }
            serde_json::to_string_pretty(&project_registry::register_project(
                home,
                Path::new(&args[1]),
                alias,
                ownership,
            )?)
            .map_err(|e| e.to_string())
        }
        _ => Err(HELP.to_owned()),
    }
}
