//! Thin CLI for the canonical filesystem project owner.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use multplx_domain::{
    project_discovery,
    project_registry::{self, CheckoutOwnership},
};

const HELP: &str = "Usage: mx project <register|list|resolve|repair|forget|roots|configure|discover>\n\n  register PATH [--alias NAME] [--managed]\n      Remember an existing Git checkout; default ownership is user-owned.\n      Registration never changes checkout files, branches, index or remotes.\n  list\n      Print remembered projects, including unavailable locations, as JSON.\n  resolve SELECTOR\n      Resolve an exact ID, alias, name or canonical path to a task-ready binding.\n      Ambiguity lists candidate paths and missing/replaced paths fail for repair.\n  repair CHECKOUT_ID PATH\n      Update a moved checkout only when its filesystem and Git identities match.\n  forget CHECKOUT_ID\n      Remove its registry location only; never delete checkout files.\n  roots list\n  roots add PATH [--depth N]\n  roots remove PATH\n      Configure optional scan roots independently of the launch directory.\n  configure [--follow-symlinks|--no-follow-symlinks] [--exclude NAME]...\n      Set symlink traversal and, when supplied, replace excluded directory names.\n  discover [--refresh]\n      Read the persistent discovery cache, or refresh it with bounded traversal.\n      Refresh publishes bounded progress during and after every root. Candidates stay\n      unregistered until explicitly selected. Unversioned candidates are listed\n      honestly and cannot produce task/worktree bindings.\n\nRegistration, repair and resolution preserve stable per-task project/checkout/base\nidentity. Remote-free Git repositories remain task-ready. Linked worktrees keep\ndistinct checkout IDs. Discovery never executes project content or loads instructions.\n";

pub fn run(args: &[OsString]) -> i32 {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return 0;
    }
    let (_, home, data) = crate::active_paths();
    let config = std::env::var_os("MX_CONFIG_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("config"));
    match execute(&home, &data, &config, args) {
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

fn execute(home: &Path, data: &Path, config: &Path, args: &[OsString]) -> Result<String, String> {
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
        "repair" if args.len() == 3 => {
            serde_json::to_string_pretty(&project_registry::repair_checkout(
                home,
                args[1].to_str().ok_or("checkout ID must be UTF-8")?,
                Path::new(&args[2]),
            )?)
            .map_err(|e| e.to_string())
        }
        "roots" if args.len() == 2 && args[1] == "list" => {
            serde_json::to_string_pretty(&project_discovery::read_config(config)?)
                .map_err(|e| e.to_string())
        }
        "roots" if args.len() >= 3 && args[1] == "add" => {
            let mut depth = 4_u16;
            let mut index = 3;
            while index < args.len() {
                if args[index] != "--depth" || index + 1 >= args.len() {
                    return Err(format!("unexpected roots add argument {:?}", args[index]));
                }
                depth = args[index + 1]
                    .to_str()
                    .ok_or("--depth must be UTF-8")?
                    .parse()
                    .map_err(|_| "--depth must be an integer from 0 to 32")?;
                index += 2;
            }
            serde_json::to_string_pretty(&project_discovery::add_root(
                config,
                Path::new(&args[2]),
                depth,
            )?)
            .map_err(|e| e.to_string())
        }
        "roots" if args.len() == 3 && args[1] == "remove" => serde_json::to_string_pretty(
            &project_discovery::remove_root(config, Path::new(&args[2]))?,
        )
        .map_err(|e| e.to_string()),
        "configure" => {
            let mut follow = None;
            let mut exclusions = Vec::new();
            let mut saw_exclusions = false;
            let mut index = 1;
            while index < args.len() {
                match args[index].to_str() {
                    Some("--follow-symlinks") if follow.is_none() => follow = Some(true),
                    Some("--no-follow-symlinks") if follow.is_none() => follow = Some(false),
                    Some("--exclude") => {
                        index += 1;
                        exclusions.push(
                            args.get(index)
                                .and_then(|value| value.to_str())
                                .ok_or("--exclude requires a UTF-8 directory name")?
                                .to_owned(),
                        );
                        saw_exclusions = true;
                    }
                    _ => return Err(format!("unexpected configure argument {:?}", args[index])),
                }
                index += 1;
            }
            if follow.is_none() && !saw_exclusions {
                return Err("configure requires a symlink or exclusion option".into());
            }
            serde_json::to_string_pretty(&project_discovery::set_options(
                config,
                saw_exclusions.then_some(exclusions),
                follow,
            )?)
            .map_err(|e| e.to_string())
        }
        "discover" if args.len() == 1 => {
            serde_json::to_string_pretty(&project_discovery::read_cache(data)?.unwrap_or_else(
                || project_discovery::DiscoveryCache {
                    schema_version: project_discovery::DISCOVERY_SCHEMA_VERSION,
                    status: project_discovery::ScanStatus::Complete,
                    scanned_directories: 0,
                    completed_roots: 0,
                    total_roots: 0,
                    updated_at: 0,
                    candidates: Vec::new(),
                    errors: Vec::new(),
                },
            ))
            .map_err(|e| e.to_string())
        }
        "discover" if args.len() == 2 && args[1] == "--refresh" => {
            serde_json::to_string_pretty(&project_discovery::refresh(home, config, data)?)
                .map_err(|e| e.to_string())
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
