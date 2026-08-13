use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const MANIFEST: &str = include_str!("test_manifest.tsv");
const HELP: &str = "Run Multplx behavior tests with the audited resource scheduler.\n\nSelection (choose one):\n  mx test-run --all\n  mx test-run --family NAME\n  mx test-run --changed [--base REF]\n  mx test-run --lane NAME\n  mx test-run --proven-isolated\n  mx test-run tests/name.test.sh [more scripts]\n\nInspection:\n  mx test-run --list --all\n  mx test-run --list-families\n  mx test-run --list-lanes\n  mx test-run --list-resources --all\n  mx test-run --check-coverage\n\nAggregation:\n  mx test-run --aggregate-json OUT INPUT...\n  mx test-run --compare-json SERIAL ACCELERATED\n";
const FAMILIES: &[&str] = &[
    "pure-contract-unit",
    "watcher-wake-lock",
    "real-herdr-gated",
    "daemon",
    "session-bootstrap",
    "live-harness-optin",
    "backend-dispatch",
    "pr-forge",
    "afk",
    "snapshot-catchup",
    "cmux",
    "unclassified",
];
const LANES: &[&str] = &[
    "portable-parallel-1",
    "portable-parallel-2",
    "portable-serial",
    "real-herdr-gated",
];

#[derive(Clone, Debug)]
struct ManifestRow {
    path: String,
    resources: String,
}

fn manifest() -> Vec<ManifestRow> {
    let mut rows = MANIFEST
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(path, resources)| ManifestRow {
            path: path.to_owned(),
            resources: resources.to_owned(),
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.path.cmp(&right.path));
    rows
}

fn canonical_manifest() -> String {
    manifest()
        .into_iter()
        .map(|row| format!("{}\t{}\n", row.path, row.resources))
        .collect()
}

fn root() -> Result<PathBuf, String> {
    if let Some(root) =
        std::env::var_os("MX_RUST_SOURCE_ROOT").or_else(|| std::env::var_os("MX_ROOT_OVERRIDE"))
    {
        return PathBuf::from(root)
            .canonicalize()
            .map_err(|error| error.to_string());
    }
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let output = Command::new("git")
        .arg("-C")
        .arg(&cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("current directory is not a git checkout".to_owned());
    }
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
        .canonicalize()
        .map_err(|error| error.to_string())
}

fn family(path: &str) -> &'static str {
    let name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path);
    match name {
        "mx-arm-pretool-check.test.sh"
        | "mx-ask-user-authority.test.sh"
        | "mx-brief.test.sh"
        | "mx-cursor-adapter.test.sh"
        | "mx-backlog-lib.test.sh"
        | "mx-calm-pi-extension.test.sh"
        | "mx-lock-override.test.sh"
        | "mx-maintainer-override.test.sh"
        | "mx-maintainer-translation-contract.test.sh"
        | "mx-cd-pretool-check.test.sh"
        | "mx-composer-ghost.test.sh"
        | "mx-composer-lib.test.sh"
        | "mx-actor-state.test.sh"
        | "mx-decision-hold-lifecycle.test.sh"
        | "mx-documentation-audiences.test.sh"
        | "mx-ensure-agents-md.test.sh"
        | "mx-naming.test.sh"
        | "mx-herdr-lab.test.sh"
        | "mx-instruction-owners.test.sh"
        | "mx-install-herdr.test.sh"
        | "mx-deep-review-lib.test.sh"
        | "mx-deep-review.test.sh"
        | "mx-deep-review-config-contract.test.sh"
        | "mx-doctor.test.sh"
        | "mx-journal.test.sh"
        | "mx-launcher.test.sh"
        | "mx-launcher-shell.test.sh"
        | "mx-timeline.test.sh"
        | "mx-report.test.sh"
        | "mx-report-mcp.test.sh"
        | "mx-signal-precedence.test.sh"
        | "mx-removed-deps.test.sh"
        | "mx-operational-input.test.sh"
        | "mx-pi-primary-types.test.sh"
        | "mx-send-popup-settle.test.sh"
        | "mx-send-settle.test.sh"
        | "mx-stow-contract.test.sh"
        | "mx-subagent-pretool-check.test.sh"
        | "mx-supervision-instructions.test.sh"
        | "mx-tmux-submit-busy.test.sh"
        | "mx-transition-lib.test.sh"
        | "mx-viz.test.sh"
        | "mx-vplan.test.sh"
        | "mx-workflow-lib.test.sh"
        | "mx-workflow.test.sh"
        | "mx-upstream-diff.test.sh"
        | "mx-test-run.test.sh"
        | "mx-test-isolation-proof.test.sh"
        | "mx-test-split-parity.test.sh" => "pure-contract-unit",
        "mx-daemon.test.sh"
        | "mx-guard-stale-banner.test.sh"
        | "mx-pi-watch-extension.test.sh"
        | "mx-nudge.test.sh"
        | "mx-supervision-events.test.sh"
        | "mx-turnend-guard.test.sh"
        | "mx-wake-daemon-lifecycle-e2e.test.sh"
        | "mx-wake-queue.test.sh"
        | "mx-watch-checkpoint.test.sh"
        | "mx-watch-triage.test.sh"
        | "mx-watcher-lock.test.sh" => "watcher-wake-lock",
        "mx-afk-inject-herdr-e2e.test.sh"
        | "mx-afk-launch.test.sh"
        | "mx-backend-autodetect-smoke.test.sh"
        | "mx-backend-herdr-eventwait-smoke.test.sh"
        | "mx-backend-herdr-presentation-e2e.test.sh"
        | "mx-backend-herdr-prune-safety-e2e.test.sh"
        | "mx-backend-herdr-respawn-idem-e2e.test.sh"
        | "mx-herdr-session-cleanup-e2e.test.sh"
        | "mx-backend-herdr-smoke.test.sh"
        | "mx-backend-herdr-workspace-per-home-e2e.test.sh" => "real-herdr-gated",
        "mx-backlog-handoff.test.sh"
        | "mx-daemon-harness-model-resolution.test.sh"
        | "mx-daemon-harness-reread-retry.test.sh"
        | "mx-daemon-harness-spawn-config.test.sh"
        | "mx-daemon-lifecycle-e2e.test.sh"
        | "mx-daemon-liveness.test.sh"
        | "mx-daemon-safety.test.sh"
        | "mx-daemon-sync.test.sh"
        | "mx-send-daemon-marker.test.sh"
        | "mx-shared-maintainer-inheritance.test.sh" => "daemon",
        "mx-bootstrap.test.sh"
        | "mx-system-sync.test.sh"
        | "mx-gate-refuse.test.sh"
        | "mx-gotmp.test.sh"
        | "mx-session-start-digest-render.test.sh"
        | "mx-session-start-lock-bootstrap.test.sh"
        | "mx-session-start-process-liveness.test.sh"
        | "mx-sessionstart-nudge.test.sh"
        | "mx-tangle-guard.test.sh"
        | "mx-update.test.sh" => "session-bootstrap",
        "mx-afk-pi-herdr-return-e2e.test.sh"
        | "mx-codex-continuity-live-e2e.test.sh"
        | "mx-cursor-live-e2e.test.sh"
        | "mx-launcher-live-e2e.test.sh"
        | "mx-pi-primary-live-e2e.test.sh"
        | "mx-send-daemon-marker-herdr-e2e.test.sh" => "live-harness-optin",
        "mx-backend-herdr.test.sh"
        | "mx-backend-tmux-smoke.test.sh"
        | "mx-backend.test.sh"
        | "mx-dispatch-queue.test.sh"
        | "mx-herdr-session-cleanup.test.sh"
        | "mx-send-strict.test.sh"
        | "mx-spawn-batch.test.sh"
        | "mx-spawn-dispatch-profile.test.sh"
        | "mx-spawn-worktree-settle.test.sh"
        | "mx-headroom.test.sh" => "backend-dispatch",
        "mx-pr-check-security-fault-quarantine.test.sh"
        | "mx-pr-check-security-parser-entrypoints.test.sh"
        | "mx-pr-check-security-publication-migration.test.sh"
        | "mx-pr-check-security-retirement-teardown.test.sh"
        | "mx-pr-merge.test.sh"
        | "mx-push-service.test.sh"
        | "mx-review-diff.test.sh"
        | "mx-teardown.test.sh" => "pr-forge",
        "mx-afk-inject-e2e.test.sh" | "mx-afk-return.test.sh" => "afk",
        "mx-status-snapshot-catchup-forge.test.sh"
        | "mx-status-snapshot-landed-bounds.test.sh"
        | "mx-status-snapshot-projection-reconciliation.test.sh"
        | "mx-system-snapshot-view.test.sh" => "snapshot-catchup",
        "mx-backend-cmux.test.sh" | "mx-backend-cmux-smoke.test.sh" => "cmux",
        _ => "unclassified",
    }
}

fn expected_skip(family: &str) -> &'static str {
    match family {
        "real-herdr-gated" => "herdr",
        "live-harness-optin" => "optin-env",
        "cmux" | "snapshot-catchup" => "optional-binary",
        _ => "none",
    }
}

fn inventory(root: &Path) -> Result<Vec<String>, String> {
    let mut result = fs::read_dir(root.join("tests"))
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| name.ends_with(".test.sh"))
        .map(|name| format!("tests/{name}"))
        .collect::<Vec<_>>();
    result.sort();
    Ok(result)
}

fn resources(path: &str) -> String {
    manifest()
        .into_iter()
        .find(|row| row.path == path)
        .map_or_else(|| "global".to_owned(), |row| row.resources)
}

fn resource_conflict(left: &str, right: &str) -> bool {
    if left == "global" || right == "global" {
        return true;
    }
    if left == "none" || right == "none" {
        return false;
    }
    let right = right.split(',').collect::<HashSet<_>>();
    left.split(',').any(|resource| right.contains(resource))
}

fn scheduler_pool(root: &Path) -> Result<Vec<String>, String> {
    Ok(inventory(root)?
        .into_iter()
        .filter(|path| {
            let resource = resources(path);
            family(path) != "real-herdr-gated"
                && !resource
                    .split(',')
                    .any(|value| matches!(value, "global" | "live-harness"))
        })
        .collect())
}

fn estimates(root: &Path) -> BTreeMap<String, u64> {
    let Ok(bytes) = fs::read(root.join("docs/mx-test-performance-baseline.json")) else {
        return BTreeMap::new();
    };
    let Ok(document) = serde_json::from_slice::<Value>(&bytes) else {
        return BTreeMap::new();
    };
    let mut values = BTreeMap::new();
    if let Some(rows) = document.get("scripts").and_then(Value::as_array) {
        for row in rows {
            if let (Some(path), Some(duration)) = (
                row.get("path").and_then(Value::as_str),
                row.get("duration_ms").and_then(Value::as_u64),
            ) {
                values.insert(path.to_owned(), duration.max(1));
            }
        }
    }
    if let Some(rows) = document
        .get("scheduler_estimates_ms")
        .and_then(Value::as_object)
    {
        for (path, duration) in rows {
            if let Some(duration) = duration.as_u64() {
                values.insert(path.clone(), duration.max(1));
            }
        }
    }
    values
}

fn portable_shards(root: &Path) -> Result<[Vec<String>; 2], String> {
    let estimates = estimates(root);
    let mut pool = scheduler_pool(root)?;
    pool.sort_by(|left, right| {
        estimates
            .get(right)
            .unwrap_or(&1000)
            .cmp(estimates.get(left).unwrap_or(&1000))
            .then_with(|| left.cmp(right))
    });
    let mut shards = [Vec::new(), Vec::new()];
    let mut totals = [0_u64, 0_u64];
    for path in pool {
        let target = usize::from(totals[1] < totals[0]);
        totals[target] += estimates.get(&path).copied().unwrap_or(1000);
        shards[target].push(path);
    }
    Ok(shards)
}

fn lane(root: &Path, name: &str) -> Result<Vec<String>, String> {
    match name {
        "portable-parallel-1" => Ok(portable_shards(root)?[0].clone()),
        "portable-parallel-2" => Ok(portable_shards(root)?[1].clone()),
        "portable-serial" => Ok(inventory(root)?
            .into_iter()
            .filter(|path| {
                let resource = resources(path);
                family(path) != "real-herdr-gated"
                    && resource
                        .split(',')
                        .any(|value| matches!(value, "global" | "live-harness"))
            })
            .collect()),
        "real-herdr-gated" => Ok(inventory(root)?
            .into_iter()
            .filter(|path| family(path) == "real-herdr-gated")
            .collect()),
        _ => Err(format!("unknown lane '{name}' (see --list-lanes)")),
    }
}

fn coverage(root: &Path) -> Result<String, String> {
    let all = inventory(root)?.into_iter().collect::<BTreeSet<_>>();
    let rows = manifest();
    let declared = rows
        .iter()
        .map(|row| row.path.clone())
        .collect::<BTreeSet<_>>();
    if rows.len() != declared.len() {
        return Err("coverage guard: duplicate scripts in resource manifest".to_owned());
    }
    if all != declared {
        return Err(
            "coverage guard: resource manifest must equal tests/*.test.sh exactly".to_owned(),
        );
    }
    for row in &rows {
        if row.resources.is_empty()
            || (row.resources.contains("none,") || row.resources.contains("global,"))
        {
            return Err("coverage guard: invalid resource manifest row".to_owned());
        }
    }
    let shards = portable_shards(root)?;
    let serial = lane(root, "portable-serial")?;
    let herdr = lane(root, "real-herdr-gated")?;
    let mut union = BTreeSet::new();
    for path in shards[0]
        .iter()
        .chain(&shards[1])
        .chain(&serial)
        .chain(&herdr)
    {
        if !union.insert(path.clone()) {
            return Err(format!(
                "coverage guard: duplicate script across lanes: {path}"
            ));
        }
    }
    if union != all {
        return Err("coverage guard: union of portable shards + portable serial + Herdr must equal tests/*.test.sh".to_owned());
    }
    Ok(format!(
        "MX_TEST_COVERAGE ok total={} accelerated={} serial={} herdr={} manifest={}",
        all.len(),
        shards[0].len() + shards[1].len(),
        serial.len(),
        herdr.len(),
        rows.len()
    ))
}

#[derive(Default)]
struct Options {
    mode: Option<String>,
    family: Option<String>,
    lane: Option<String>,
    base: String,
    list: bool,
    list_resources: bool,
    json: Option<PathBuf>,
    jobs: Option<String>,
    exclude: Vec<String>,
    fail_skip: Option<String>,
    scripts: Vec<String>,
    aggregate: Option<PathBuf>,
}

fn die(message: impl AsRef<str>) -> i32 {
    eprintln!("mx-test-run: {}", message.as_ref());
    2
}

fn parse(args: &[OsString]) -> Result<Option<Options>, String> {
    let mut options = Options {
        base: "origin/main".to_owned(),
        ..Options::default()
    };
    let mut index = 0;
    let selection = |options: &mut Options, mode: &str| -> Result<(), String> {
        if options.mode.is_some() {
            Err("only one selection mode is allowed".to_owned())
        } else {
            options.mode = Some(mode.to_owned());
            Ok(())
        }
    };
    while index < args.len() {
        let value = args[index].to_string_lossy();
        let take = |index: &mut usize, label: &str| -> Result<String, String> {
            *index += 1;
            args.get(*index)
                .map(|value| value.to_string_lossy().into_owned())
                .ok_or_else(|| format!("{label} requires a value"))
        };
        match value.as_ref() {
            "-h" | "--help" => return Ok(None),
            "--all" => selection(&mut options, "all")?,
            "--family" => {
                selection(&mut options, "family")?;
                options.family = Some(take(&mut index, "--family")?);
            }
            "--lane" => {
                selection(&mut options, "lane")?;
                options.lane = Some(take(&mut index, "--lane")?);
            }
            "--proven-isolated" => selection(&mut options, "proven-isolated")?,
            "--changed" => selection(&mut options, "changed")?,
            "--base" => options.base = take(&mut index, "--base")?,
            "--json" => options.json = Some(PathBuf::from(take(&mut index, "--json")?)),
            "--jobs" => options.jobs = Some(take(&mut index, "--jobs")?),
            "--exclude-family" => options.exclude.push(take(&mut index, "--exclude-family")?),
            "--fail-on-gate-skip" => {
                options.fail_skip = Some(take(&mut index, "--fail-on-gate-skip")?)
            }
            "--list" => options.list = true,
            "--list-resources" => options.list_resources = true,
            "--list-families" => options.mode = Some("list-families".to_owned()),
            "--list-lanes" => options.mode = Some("list-lanes".to_owned()),
            "--check-coverage" => options.mode = Some("coverage".to_owned()),
            "--aggregate-json" => {
                selection(&mut options, "aggregate")?;
                options.aggregate = Some(PathBuf::from(take(&mut index, "--aggregate-json")?));
            }
            "--compare-json" => selection(&mut options, "compare")?,
            _ if value.starts_with("--family=") => {
                selection(&mut options, "family")?;
                options.family = Some(value[9..].to_owned());
            }
            _ if value.starts_with("--lane=") => {
                selection(&mut options, "lane")?;
                options.lane = Some(value[7..].to_owned());
            }
            _ if value.starts_with("--base=") => options.base = value[7..].to_owned(),
            _ if value.starts_with("--json=") => options.json = Some(PathBuf::from(&value[7..])),
            _ if value.starts_with("--jobs=") => options.jobs = Some(value[7..].to_owned()),
            _ if value.starts_with("--exclude-family=") => {
                options.exclude.push(value[17..].to_owned())
            }
            _ if value.starts_with("--fail-on-gate-skip=") => {
                options.fail_skip = Some(value[20..].to_owned())
            }
            _ if value.starts_with('-') => return Err(format!("unknown option: {value}")),
            _ => {
                if options.mode.as_deref().is_none() {
                    options.mode = Some("scripts".to_owned());
                }
                if !matches!(
                    options.mode.as_deref(),
                    Some("scripts" | "aggregate" | "compare")
                ) {
                    return Err(format!(
                        "script paths cannot be combined with --{}",
                        options.mode.as_deref().unwrap_or("")
                    ));
                }
                options.scripts.push(value.into_owned());
            }
        }
        index += 1;
    }
    Ok(Some(options))
}

fn normalize_script(root: &Path, value: &str) -> String {
    if Path::new(value).is_absolute() {
        return value.to_owned();
    }
    let value = value.strip_prefix("./").unwrap_or(value);
    if value.ends_with(".test.sh")
        && !value.starts_with("tests/")
        && root.join("tests").join(value).is_file()
    {
        format!("tests/{value}")
    } else {
        value.to_owned()
    }
}

fn command_lines(mut command: Command) -> Result<Vec<String>, String> {
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

fn tests_referencing(root: &Path, needle: &str) -> Vec<&'static str> {
    let mut families = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(root.join("tests")) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("sh")
                || !path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.ends_with(".test.sh"))
            {
                continue;
            }
            if fs::read_to_string(&path).is_ok_and(|text| text.contains(needle)) {
                families.insert(family(path.to_str().unwrap_or("")));
            }
        }
    }
    families.into_iter().collect()
}

fn changed_families(
    root: &Path,
    path: &str,
) -> Result<(Vec<&'static str>, Option<String>), String> {
    if path.starts_with("tests/") && path.ends_with(".test.sh") {
        return Ok((
            Vec::new(),
            Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_owned),
        ));
    }
    let values = if path == "tests/mx-backend-herdr-eventwait.test.py" {
        vec!["real-herdr-gated", "backend-dispatch"]
    } else if path.contains("mx-supervisor-target-lib") {
        vec![
            "watcher-wake-lock",
            "real-herdr-gated",
            "live-harness-optin",
            "afk",
        ]
    } else if path.starts_with(".agents/skills/")
        || matches!(path, "AGENTS.md" | "CLAUDE.md" | "CONTRIBUTING.md")
    {
        vec!["pure-contract-unit"]
    } else if path == "tests/lib.sh"
        || path.starts_with("tests/")
        || path.starts_with(".claude/")
        || path.starts_with(".pi/")
    {
        tests_referencing(
            root,
            Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(path),
        )
    } else if path.starts_with("bin/mx-test-")
        || path.contains("mx-doc-audience")
        || path.starts_with("crates/")
        || path.starts_with(".github/")
        || path == ".deep-review.yaml"
    {
        vec!["pure-contract-unit"]
    } else if path.starts_with("bin/backends/herdr") || path.contains("mx-herdr") {
        vec!["real-herdr-gated", "backend-dispatch", "pure-contract-unit"]
    } else if path.starts_with("bin/") || path.contains('/') && path.ends_with(".md") {
        let found = tests_referencing(
            root,
            Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(path),
        );
        if found.is_empty() {
            return Err(format!("no changed-test mapping for source path: {path}"));
        }
        found
    } else if matches!(path, "README.md" | "LICENSE" | ".gitignore")
        || path.starts_with("docs/")
        || path.starts_with("assets/")
        || path.starts_with("plans/")
        || path.ends_with(".md")
    {
        Vec::new()
    } else {
        let found = tests_referencing(root, path);
        if found.is_empty() {
            return Err(format!("no changed-test mapping for source path: {path}"));
        }
        found
    };
    Ok((values, None))
}

fn changed(root: &Path, base: &str) -> Result<Vec<String>, String> {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", base])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!(
            "changed-file base ref not found: {base} (pass --base <ref>)"
        ));
    }
    let mut paths = BTreeSet::new();
    for arguments in [
        vec!["diff", "--name-only", &format!("{base}...HEAD")],
        vec!["diff", "--name-only", "HEAD"],
        vec!["ls-files", "--others", "--exclude-standard"],
    ] {
        let mut command = Command::new("git");
        command.arg("-C").arg(root).args(arguments);
        paths.extend(command_lines(command)?);
    }
    let all = inventory(root)?;
    let mut wanted_families = BTreeSet::new();
    let mut wanted_scripts = BTreeSet::new();
    for path in paths {
        let (families, script) = changed_families(root, &path)?;
        wanted_families.extend(families);
        if let Some(script) = script {
            wanted_scripts.insert(script);
        }
    }
    Ok(all
        .into_iter()
        .filter(|path| {
            wanted_families.contains(family(path))
                || Path::new(path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| wanted_scripts.contains(name))
        })
        .collect())
}

fn select(root: &Path, options: &Options) -> Result<(Vec<String>, String), String> {
    let mode = options.mode.as_deref().ok_or("select with --all, --family <name>, --lane <name>, --proven-isolated, --changed, or one or more script paths (see --help)")?;
    let (mut scripts, description) = match mode {
        "all" => (inventory(root)?, "all".to_owned()),
        "family" => {
            let family_name = options
                .family
                .as_deref()
                .ok_or("--family requires a name")?;
            let scripts = inventory(root)?
                .into_iter()
                .filter(|path| family(path) == family_name)
                .collect::<Vec<_>>();
            if scripts.is_empty() {
                return Err(format!("no tests mapped to family '{family_name}'"));
            }
            (scripts, format!("family={family_name}"))
        }
        "lane" => {
            let name = options.lane.as_deref().ok_or("--lane requires a name")?;
            (lane(root, name)?, format!("lane={name}"))
        }
        "proven-isolated" => (
            manifest()
                .into_iter()
                .filter(|row| row.resources == "none")
                .map(|row| row.path)
                .collect(),
            "proven-isolated".to_owned(),
        ),
        "changed" => (
            changed(root, &options.base)?,
            format!("changed:base={}", options.base),
        ),
        "scripts" => {
            let mut seen = BTreeSet::new();
            let scripts = options
                .scripts
                .iter()
                .map(|path| normalize_script(root, path))
                .filter(|path| seen.insert(path.clone()))
                .collect();
            (scripts, "scripts".to_owned())
        }
        _ => return Err("invalid selection mode".to_owned()),
    };
    scripts.retain(|path| {
        !options
            .exclude
            .iter()
            .any(|excluded| family(path) == excluded)
    });
    let mut description = description;
    if !options.exclude.is_empty() {
        description.push_str(&format!(";exclude-family={}", options.exclude.join(",")));
    }
    if let Some(token) = &options.fail_skip {
        description.push_str(&format!(";fail-on-gate-skip={token}"));
    }
    Ok((scripts, description))
}

fn now_iso() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}
fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[derive(Clone)]
struct ResultRow {
    path: String,
    family: String,
    expected: String,
    resources: String,
    code: i32,
    duration_ms: u128,
    gate_skip: bool,
    output: String,
    started: String,
    finished: String,
}

fn run_child(root: &Path, path: &str, temp: Option<&Path>) -> ResultRow {
    let started = now_iso();
    let clock = Instant::now();
    let mut command = Command::new("bash");
    command
        .arg(path)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(temp) = temp {
        command.env("TMPDIR", temp).env("TMP", temp);
    }
    for variable in [
        "MX_HOME",
        "MX_STATE_OVERRIDE",
        "MX_DATA_OVERRIDE",
        "MX_ROOT_OVERRIDE",
        "MX_PROJECTS_OVERRIDE",
        "MX_CONFIG_OVERRIDE",
        "MX_BACKEND",
        "MX_MULTICALL_EXPLICIT",
    ] {
        command.env_remove(variable);
    }
    let output = command.output();
    let (code, text) = match output {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            (output.status.code().unwrap_or(1), text)
        }
        Err(error) => (1, format!("{error}\n")),
    };
    ResultRow {
        path: path.to_owned(),
        family: family(path).to_owned(),
        expected: expected_skip(family(path)).to_owned(),
        resources: resources(path),
        code,
        duration_ms: clock.elapsed().as_millis(),
        gate_skip: code == 0
            && text
                .lines()
                .find(|line| !line.trim().is_empty())
                .is_some_and(|line| line.starts_with("skip:")),
        output: text,
        started,
        finished: now_iso(),
    }
}

struct Running {
    index: usize,
    child: Child,
    directory: tempfile::TempDir,
    output: PathBuf,
    started: String,
    clock: Instant,
}

fn finish_running(running: Running, status: ExitStatus, paths: &[String]) -> ResultRow {
    let Running {
        index,
        child: _,
        directory,
        output,
        started,
        clock,
    } = running;
    let path = &paths[index];
    let mode = fs::metadata(directory.path())
        .map(|metadata| metadata.permissions().mode() & 0o777)
        .unwrap_or(0);
    let mut code = status.code().unwrap_or(1);
    let mut text = fs::read(&output)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|error| format!("mx-test-run: cannot read worker output: {error}\n"));
    if mode != 0o700 {
        text.push_str(&format!(
            "mx-test-run: isolation failure: worker root mode is {mode:o}, expected 0700\n"
        ));
        code = 1;
    }
    // `std::fs::remove_dir_all` is pathologically slow for the large nested Git
    // fixtures used by lifecycle tests on macOS.  TempDir's Drop would also hide
    // that time after the result duration had been captured.  Delegate removal
    // of this exact tempfile-owned root to the platform utility, verify absence,
    // and include cleanup in the per-test and aggregate wall clocks.
    let worker_root = directory.keep();
    let safe_worker = worker_root
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with("mx-test-worker."));
    let cleanup_ok = safe_worker
        && Command::new("/bin/rm")
            .arg("-rf")
            .arg(&worker_root)
            .status()
            .is_ok_and(|cleanup| cleanup.success())
        && !worker_root.exists();
    if !cleanup_ok {
        text.push_str(&format!(
            "mx-test-run: isolation failure: could not remove worker root {}\n",
            worker_root.display()
        ));
        code = 1;
    }
    ResultRow {
        path: path.clone(),
        family: family(path).to_owned(),
        expected: expected_skip(family(path)).to_owned(),
        resources: resources(path),
        code,
        duration_ms: clock.elapsed().as_millis(),
        gate_skip: code == 0
            && text
                .lines()
                .find(|line| !line.trim().is_empty())
                .is_some_and(|line| line.starts_with("skip:")),
        output: text,
        started,
        finished: now_iso(),
    }
}

fn parallel(root: &Path, paths: &[String], jobs: usize) -> Vec<ResultRow> {
    let estimate = estimates(root);
    let mut pending = (0..paths.len()).collect::<BTreeSet<_>>();
    let mut running = Vec::<Running>::new();
    let mut results = BTreeMap::new();
    while results.len() < paths.len() {
        while running.len() < jobs {
            let candidate = pending
                .iter()
                .copied()
                .filter(|index| {
                    running.iter().all(|active| {
                        !resource_conflict(
                            &resources(&paths[*index]),
                            &resources(&paths[active.index]),
                        )
                    })
                })
                .max_by_key(|index| {
                    (
                        estimate.get(&paths[*index]).copied().unwrap_or(1000),
                        std::cmp::Reverse(*index),
                    )
                });
            let Some(index) = candidate else {
                break;
            };
            pending.remove(&index);
            let directory = tempfile::Builder::new()
                .prefix("mx-test-worker.")
                .tempdir()
                .expect("worker temp");
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).ok();
            let private = directory.path().join("tmp");
            fs::create_dir(&private).ok();
            fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).ok();
            let output = directory.path().join("output.log");
            let Ok(stdout) = fs::File::create(&output) else {
                pending.remove(&index);
                results.insert(
                    index,
                    ResultRow {
                        path: paths[index].clone(),
                        family: family(&paths[index]).to_owned(),
                        expected: expected_skip(family(&paths[index])).to_owned(),
                        resources: resources(&paths[index]),
                        code: 1,
                        duration_ms: 0,
                        gate_skip: false,
                        output: "mx-test-run: cannot create worker output\n".to_owned(),
                        started: now_iso(),
                        finished: now_iso(),
                    },
                );
                continue;
            };
            let stderr = stdout.try_clone().expect("clone worker output");
            let mut command = Command::new("bash");
            command
                .arg(&paths[index])
                .current_dir(root)
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr))
                .env("TMPDIR", &private)
                .env("TMP", &private);
            for variable in [
                "MX_HOME",
                "MX_STATE_OVERRIDE",
                "MX_DATA_OVERRIDE",
                "MX_ROOT_OVERRIDE",
                "MX_PROJECTS_OVERRIDE",
                "MX_CONFIG_OVERRIDE",
                "MX_BACKEND",
                "MX_MULTICALL_EXPLICIT",
            ] {
                command.env_remove(variable);
            }
            match command.spawn() {
                Ok(child) => running.push(Running {
                    index,
                    child,
                    directory,
                    output,
                    started: now_iso(),
                    clock: Instant::now(),
                }),
                Err(error) => {
                    results.insert(
                        index,
                        ResultRow {
                            path: paths[index].clone(),
                            family: family(&paths[index]).to_owned(),
                            expected: expected_skip(family(&paths[index])).to_owned(),
                            resources: resources(&paths[index]),
                            code: 1,
                            duration_ms: 0,
                            gate_skip: false,
                            output: format!("{error}\n"),
                            started: now_iso(),
                            finished: now_iso(),
                        },
                    );
                }
            }
        }
        let finished = running
            .iter_mut()
            .enumerate()
            .find_map(|(position, running)| {
                running
                    .child
                    .try_wait()
                    .ok()
                    .flatten()
                    .map(|status| (position, status))
            });
        if let Some((position, status)) = finished {
            let running = running.swap_remove(position);
            results.insert(running.index, finish_running(running, status, paths));
        } else if !running.is_empty() {
            thread::sleep(Duration::from_millis(10));
        } else if !pending.is_empty() {
            break;
        }
    }
    results.into_values().collect()
}

fn artifact(
    selection: &str,
    jobs: usize,
    started: &str,
    finished: &str,
    duration_ms: u128,
    rows: &[ResultRow],
) -> Value {
    let mut family_rows = BTreeMap::<String, (usize, u128, usize)>::new();
    for row in rows {
        let entry = family_rows.entry(row.family.clone()).or_default();
        entry.0 += 1;
        entry.1 += row.duration_ms;
        entry.2 += usize::from(row.code != 0);
    }
    json!({
        "run_id": format!("mx-test-run-{}-{}", epoch_ms(), std::process::id()), "started_at": started, "finished_at": finished,
        "selection": selection, "scheduler": {"jobs": jobs, "resource_aware": jobs > 1},
        "summary": {"total": rows.len(), "failed": rows.iter().filter(|row| row.code != 0).count(), "skipped_gate": rows.iter().filter(|row| row.gate_skip).count(), "duration_ms": duration_ms},
        "scripts": rows.iter().map(|row| json!({"path": row.path, "family": row.family, "expected_gate_skip": row.expected, "resources": if matches!(row.resources.as_str(), "none" | "global") { vec![row.resources.clone()] } else { row.resources.split(',').map(str::to_owned).collect() }, "duration_ms": row.duration_ms, "exit": row.code, "gate_skip": row.gate_skip, "assertions": row.output.lines().filter(|line| line.starts_with("ok - ") || line.starts_with("not ok - ")).collect::<Vec<_>>() })).collect::<Vec<_>>(),
        "families": family_rows.into_iter().map(|(name, (count, duration, failed))| json!({"name": name, "count": count, "duration_ms": duration, "failed": failed})).collect::<Vec<_>>()
    })
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    multplx_core::filesystem::atomic_replace(path, &bytes, 0o600).map_err(|error| error.to_string())
}

fn aggregate(out: &Path, inputs: &[String]) -> Result<String, String> {
    if inputs.is_empty() {
        return Err("--aggregate-json requires at least one input timing JSON".to_owned());
    }
    let mut lanes = Vec::new();
    let mut scripts = Vec::new();
    let mut total = 0_u64;
    let mut failed = 0_u64;
    let mut skipped = 0_u64;
    let mut wall = 0_u64;
    for path in inputs {
        let document: Value = serde_json::from_slice(
            &fs::read(path).map_err(|_| format!("aggregate input not found: {path}"))?,
        )
        .map_err(|error| error.to_string())?;
        let summary = document.get("summary").cloned().unwrap_or(Value::Null);
        total += summary.get("total").and_then(Value::as_u64).unwrap_or(0);
        failed += summary.get("failed").and_then(Value::as_u64).unwrap_or(0);
        skipped += summary
            .get("skipped_gate")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        wall = wall.max(
            summary
                .get("duration_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
        lanes.push(json!({"path": path, "run_id": document.get("run_id"), "selection": document.get("selection"), "started_at": document.get("started_at"), "finished_at": document.get("finished_at"), "summary": summary}));
        if let Some(rows) = document.get("scripts").and_then(Value::as_array) {
            for row in rows {
                let mut row = row.clone();
                if let Some(object) = row.as_object_mut() {
                    object.insert(
                        "lane_selection".to_owned(),
                        document.get("selection").cloned().unwrap_or(Value::Null),
                    );
                    object.insert(
                        "lane_run_id".to_owned(),
                        document.get("run_id").cloned().unwrap_or(Value::Null),
                    );
                }
                scripts.push(row);
            }
        }
    }
    scripts.sort_by(|left, right| {
        right
            .get("duration_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .cmp(&left.get("duration_ms").and_then(Value::as_u64).unwrap_or(0))
            .then_with(|| {
                left.get("path")
                    .and_then(Value::as_str)
                    .cmp(&right.get("path").and_then(Value::as_str))
            })
    });
    let slowest = scripts.iter().take(15).cloned().collect::<Vec<_>>();
    write_json(
        out,
        &json!({"kind":"aggregate", "lanes": lanes, "summary":{"lanes": lanes.len(), "total":total, "failed":failed, "skipped_gate":skipped, "critical_path_duration_ms":wall}, "scripts":scripts, "slowest":slowest}),
    )?;
    Ok(format!(
        "MX_TEST_AGGREGATE lanes={} total={total} failed={failed} skipped_gate={skipped} critical_path_duration_ms={wall}",
        lanes.len()
    ))
}

fn normalized(document: &Value) -> BTreeMap<String, (i64, bool, BTreeMap<String, usize>)> {
    let mut output = BTreeMap::new();
    for row in document
        .get("scripts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut assertions = BTreeMap::new();
        for assertion in row
            .get("assertions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            *assertions.entry(assertion.to_owned()).or_default() += 1;
        }
        output.insert(
            row.get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            (
                row.get("exit").and_then(Value::as_i64).unwrap_or(0),
                row.get("gate_skip")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                assertions,
            ),
        );
    }
    output
}

fn compare(inputs: &[String]) -> Result<String, String> {
    if inputs.len() != 2 {
        return Err("--compare-json requires exactly two timing JSON paths".to_owned());
    }
    let documents = inputs
        .iter()
        .map(|path| {
            fs::read(path)
                .map_err(|_| format!("compare input not found: {path}"))
                .and_then(|bytes| {
                    serde_json::from_slice::<Value>(&bytes).map_err(|error| error.to_string())
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let left = normalized(&documents[0]);
    let right = normalized(&documents[1]);
    let summaries_equal = ["failed", "skipped_gate"].iter().all(|field| {
        documents[0]
            .pointer(&format!("/summary/{field}"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
            == documents[1]
                .pointer(&format!("/summary/{field}"))
                .and_then(Value::as_i64)
                .unwrap_or(0)
    });
    if left != right || !summaries_equal {
        return Err("MX_TEST_PARITY mismatch".to_owned());
    }
    let assertion_count = left
        .values()
        .map(|(_, _, assertions)| assertions.values().sum::<usize>())
        .sum::<usize>();
    Ok(format!(
        "MX_TEST_PARITY ok scripts={} assertions={assertion_count}",
        left.len()
    ))
}

pub(super) fn run(args: &[OsString]) -> i32 {
    let root = match root() {
        Ok(root) => root,
        Err(error) => return die(error),
    };
    let options = match parse(args) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{HELP}");
            return 0;
        }
        Err(error) => return die(error),
    };
    match options.mode.as_deref() {
        Some("list-families") => {
            for value in FAMILIES {
                println!("{value}");
            }
            return 0;
        }
        Some("list-lanes") => {
            for value in LANES {
                println!("{value}");
            }
            return 0;
        }
        Some("coverage") => {
            return match coverage(&root) {
                Ok(output) => {
                    println!("{output}");
                    0
                }
                Err(error) => {
                    eprintln!("mx-test-run: {error}");
                    1
                }
            };
        }
        Some("aggregate") => {
            return match aggregate(
                options.aggregate.as_deref().unwrap_or(Path::new("")),
                &options.scripts,
            ) {
                Ok(output) => {
                    println!("{output}");
                    0
                }
                Err(error) => die(error),
            };
        }
        Some("compare") => {
            return match compare(&options.scripts) {
                Ok(output) => {
                    println!("{output}");
                    0
                }
                Err(error) => {
                    println!("{error}");
                    1
                }
            };
        }
        _ => {}
    }
    let (scripts, mut description) = match select(&root, &options) {
        Ok(value) => value,
        Err(error) => return die(error),
    };
    let jobs = match options.jobs.as_deref() {
        None if options.mode.as_deref() == Some("all") => std::thread::available_parallelism()
            .map_or(2, usize::from)
            .min(4),
        None => 1,
        Some("auto") => std::thread::available_parallelism()
            .map_or(2, usize::from)
            .min(4),
        Some(value) => match value.parse::<usize>() {
            Ok(value @ 1..=8) => value,
            _ => return die("--jobs must be a positive integer or auto and is capped at 8"),
        },
    };
    if jobs > 1 {
        description.push_str(&format!(";jobs={jobs}"));
    }
    if options.list_resources {
        for path in scripts {
            println!("{path}\t{}", resources(&path));
        }
        return 0;
    }
    if options.list {
        for path in scripts {
            println!("{path}");
        }
        return 0;
    }
    for path in &scripts {
        let absolute = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            root.join(path)
        };
        if !absolute.is_file() {
            return die(format!("test script not found: {path}"));
        }
    }
    if scripts.is_empty() {
        eprintln!("mx-test-run: nothing to run");
        println!("MX_TEST_SUMMARY total=0 failed=0 skipped_gate=0 duration_ms=0");
        if let Some(path) = &options.json {
            let now = now_iso();
            if let Err(error) = write_json(path, &artifact(&description, jobs, &now, &now, 0, &[]))
            {
                return die(error);
            }
        }
        return 0;
    }
    let started = now_iso();
    let clock = Instant::now();
    let mut rows = if jobs == 1 {
        scripts
            .iter()
            .map(|path| run_child(&root, path, None))
            .collect::<Vec<_>>()
    } else {
        parallel(&root, &scripts, jobs)
    };
    for row in &mut rows {
        if let Some(token) = &options.fail_skip
            && row
                .output
                .lines()
                .any(|line| line.contains(&format!("skip: {token}")))
        {
            eprintln!(
                "mx-test-run: required gate skip token seen in {}: skip: {token}",
                row.path
            );
            row.code = 1;
            row.gate_skip = false;
        }
        println!(
            "MX_TEST_BEGIN {} {} family={} expected_gate_skip={}",
            row.started, row.path, row.family, row.expected
        );
        print!("{}", row.output);
        println!(
            "MX_TEST_END {} {} exit={} duration_ms={} gate_skip={}",
            row.finished, row.path, row.code, row.duration_ms, row.gate_skip
        );
    }
    let duration = clock.elapsed().as_millis();
    let failed = rows.iter().filter(|row| row.code != 0).count();
    let skipped = rows.iter().filter(|row| row.gate_skip).count();
    println!(
        "MX_TEST_SUMMARY total={} failed={failed} skipped_gate={skipped} duration_ms={duration}",
        rows.len()
    );
    let mut families = BTreeMap::<String, (usize, u128, usize)>::new();
    for row in &rows {
        let entry = families.entry(row.family.clone()).or_default();
        entry.0 += 1;
        entry.1 += row.duration_ms;
        entry.2 += usize::from(row.code != 0);
    }
    for (name, (count, duration, failed)) in families {
        println!(
            "MX_TEST_SUMMARY_FAMILY family={name} count={count} duration_ms={duration} failed={failed}"
        );
    }
    let mut slow = rows.clone();
    slow.sort_by_key(|row| std::cmp::Reverse(row.duration_ms));
    for (index, row) in slow.iter().take(15).enumerate() {
        println!(
            "MX_TEST_SLOWEST rank={} script={} duration_ms={}",
            index + 1,
            row.path,
            row.duration_ms
        );
    }
    let finished = now_iso();
    if let Some(path) = &options.json {
        if let Err(error) = write_json(
            path,
            &artifact(&description, jobs, &started, &finished, duration, &rows),
        ) {
            return die(error);
        }
        eprintln!("mx-test-run: wrote timing artifact: {}", path.display());
    }
    i32::from(failed != 0)
}

fn proof_help() {
    eprintln!(
        "Usage: mx test-isolation-proof [--jobs N] [--repeats N] [--json path]\n       mx test-isolation-proof --list|--list-resources|--list-conflicts|--list-exclusions"
    );
}

fn proof_candidates() -> Vec<ManifestRow> {
    manifest()
        .into_iter()
        .filter(|row| {
            !row.resources.split(',').any(|value| {
                matches!(
                    value,
                    "global" | "live-harness" | "herdr-session" | "cmux-app"
                )
            })
        })
        .collect()
}
fn proof_exclusions() -> Vec<(String, &'static str)> {
    manifest()
        .into_iter()
        .filter_map(|row| {
            let reason = if row.path == "tests/mx-pr-check-security-publication-migration.test.sh"
                && row.resources.split(',').any(|value| value == "global")
            {
                "load-sensitive publication race retains its ten-second hang tripwire"
            } else if row.resources.split(',').any(|value| value == "global") {
                "global scheduler owner/self-contract"
            } else if row
                .resources
                .split(',')
                .any(|value| value == "live-harness")
            {
                "live harness opt-in is not a portable stress resource"
            } else if row
                .resources
                .split(',')
                .any(|value| value == "herdr-session")
            {
                "real Herdr remains in its dedicated owned lab lane"
            } else if row.resources.split(',').any(|value| value == "cmux-app") {
                "GUI-owned cmux resource is environment gated"
            } else {
                return None;
            };
            Some((row.path, reason))
        })
        .collect()
}

fn global_git_snapshot() -> Vec<u8> {
    let Ok(output) = Command::new("git")
        .args(["config", "--global", "--list"])
        .output()
    else {
        return Vec::new();
    };
    let mut lines = output
        .stdout
        .split(|byte| *byte == b'\n')
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    lines.sort();
    lines.concat()
}

fn proof_process_leaks(proof_root: &Path) -> Vec<String> {
    let Ok(output) = Command::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .output()
    else {
        return Vec::new();
    };
    let marker = proof_root.to_string_lossy();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(marker.as_ref()))
        .map(str::to_owned)
        .collect()
}

pub(super) fn run_isolation_proof(args: &[OsString]) -> i32 {
    let root = match root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("mx-test-isolation-proof: {error}");
            return 2;
        }
    };
    let mut jobs = 4_usize;
    let mut repeats = 2_usize;
    let mut json_path = None;
    let mut mode = "run";
    let mut index = 0;
    while index < args.len() {
        let value = args[index].to_string_lossy();
        match value.as_ref() {
            "-h" | "--help" => {
                proof_help();
                return 0;
            }
            "--jobs" | "--repeats" | "--json" => {
                index += 1;
                let Some(next) = args.get(index) else {
                    eprintln!("mx-test-isolation-proof: {value} requires a value");
                    return 2;
                };
                match value.as_ref() {
                    "--jobs" => jobs = next.to_string_lossy().parse().unwrap_or(0),
                    "--repeats" => repeats = next.to_string_lossy().parse().unwrap_or(0),
                    _ => json_path = Some(PathBuf::from(next)),
                }
            }
            "--list" => mode = "list",
            "--list-resources" => mode = "resources",
            "--list-conflicts" => mode = "conflicts",
            "--list-exclusions" => mode = "exclusions",
            _ => {
                eprintln!("mx-test-isolation-proof: unknown option: {value}");
                return 2;
            }
        }
        index += 1;
    }
    if !(1..=8).contains(&jobs) || repeats == 0 {
        eprintln!("mx-test-isolation-proof: --jobs must be 1..=8 and --repeats must be positive");
        return 2;
    }
    match mode {
        "list" => {
            for row in proof_candidates() {
                println!("{}", row.path);
            }
            return 0;
        }
        "resources" => {
            print!("{}", canonical_manifest());
            return 0;
        }
        "exclusions" => {
            for (path, reason) in proof_exclusions() {
                println!("{path}\t{reason}");
            }
            return 0;
        }
        "conflicts" => {
            let rows = manifest();
            for left in 0..rows.len() {
                for right in left + 1..rows.len() {
                    if resource_conflict(&rows[left].resources, &rows[right].resources) {
                        let shared = if rows[left].resources == "global"
                            || rows[right].resources == "global"
                        {
                            "global".to_owned()
                        } else {
                            let right_set =
                                rows[right].resources.split(',').collect::<BTreeSet<_>>();
                            rows[left]
                                .resources
                                .split(',')
                                .filter(|value| right_set.contains(value))
                                .collect::<Vec<_>>()
                                .join(",")
                        };
                        println!("{}\t{}\t{}", rows[left].path, rows[right].path, shared);
                    }
                }
            }
            return 0;
        }
        _ => {}
    }
    let candidates = proof_candidates();
    let proof_root = match tempfile::Builder::new()
        .prefix("mx-isolation-proof.")
        .tempdir()
    {
        Ok(root) => root,
        Err(error) => {
            eprintln!("mx-test-isolation-proof: cannot create proof root: {error}");
            return 1;
        }
    };
    if let Err(error) = fs::set_permissions(proof_root.path(), fs::Permissions::from_mode(0o700)) {
        eprintln!("mx-test-isolation-proof: cannot secure proof root: {error}");
        return 1;
    }
    let git_before = global_git_snapshot();
    let started = now_iso();
    let clock = Instant::now();
    let mut failed = 0;
    let mut known_failure_observations = 0;
    let mut rounds = Vec::new();
    let mut scripts = Vec::new();
    println!(
        "MX_ISOLATION_BEGIN {started} concurrency={jobs} candidates={} repeats={repeats}",
        candidates.len()
    );
    for repeat in 1..=repeats {
        let path = proof_root.path().join(format!("round-{repeat}.json"));
        let round_clock = Instant::now();
        let binary = std::env::current_exe().unwrap();
        let output = Command::new(binary)
            .args(["test-run", "--jobs", &jobs.to_string(), "--json"])
            .arg(&path)
            .args(candidates.iter().map(|row| row.path.as_str()))
            .current_dir(&root)
            .env("MX_MULTICALL_EXPLICIT", "1")
            .env("TMPDIR", proof_root.path())
            .env("TMP", proof_root.path())
            .output();
        let runner_exit = output
            .as_ref()
            .ok()
            .and_then(|output| output.status.code())
            .unwrap_or(1);
        let mut contract_exit = runner_exit;
        if let Ok(bytes) = fs::read(&path)
            && let Ok(document) = serde_json::from_slice::<Value>(&bytes)
        {
            let known = fs::read(root.join("docs/mx-test-performance-baseline.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|value| {
                    value
                        .pointer("/known_failure/path")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            let failed_paths = document
                .get("scripts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|row| row.get("exit").and_then(Value::as_i64).unwrap_or(0) != 0)
                .filter_map(|row| row.get("path").and_then(Value::as_str))
                .collect::<Vec<_>>();
            let unexpected = failed_paths
                .iter()
                .any(|path| Some(*path) != known.as_deref());
            known_failure_observations += failed_paths
                .iter()
                .filter(|path| Some(**path) == known.as_deref())
                .count();
            if !unexpected {
                contract_exit = 0;
            }
            for row in document
                .get("scripts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let mut row = row.clone();
                if let Some(object) = row.as_object_mut() {
                    object.insert("repeat".to_owned(), json!(repeat));
                }
                scripts.push(row);
            }
            rounds.push(json!({"repeat":repeat,"exit":contract_exit,"runner_exit":runner_exit,"duration_ms":round_clock.elapsed().as_millis(),"run_id":document.get("run_id")}));
        }
        if contract_exit != 0 {
            failed += 1;
            if let Ok(output) = output {
                eprint!("{}", String::from_utf8_lossy(&output.stdout));
                eprint!("{}", String::from_utf8_lossy(&output.stderr));
            }
        }
        println!(
            "MX_ISOLATION_ROUND_END repeat={repeat} exit={contract_exit} runner_exit={runner_exit} duration_ms={}",
            round_clock.elapsed().as_millis()
        );
    }
    let duration = clock.elapsed().as_millis();
    let mut leaks = 0;
    if global_git_snapshot() != git_before {
        eprintln!("mx-test-isolation-proof: global git config changed during proof");
        leaks += 1;
    }
    let process_leaks = proof_process_leaks(proof_root.path());
    if !process_leaks.is_empty() {
        eprintln!("mx-test-isolation-proof: leaked proof-owned processes:");
        for process in &process_leaks {
            eprintln!("{process}");
        }
        leaks += 1;
    }
    if let Some(path) = json_path {
        let canonical_manifest = canonical_manifest();
        let hash = format!("{:x}", Sha256::digest(canonical_manifest.as_bytes()));
        let resources_json = manifest().into_iter().map(|row| json!({"path":row.path,"resources":row.resources.split(',').collect::<Vec<_>>()})).collect::<Vec<_>>();
        let conflict_pairs = {
            let rows = manifest();
            (0..rows.len())
                .flat_map(|left| (left + 1..rows.len()).map(move |right| (left, right)))
                .filter(|(left, right)| {
                    resource_conflict(&rows[*left].resources, &rows[*right].resources)
                })
                .count()
        };
        let document = json!({"kind":"resource-isolation-proof","started_at":started,"finished_at":now_iso(),"concurrency":jobs,"repeats":repeats,"manifest_sha256":hash,"resource_manifest":resources_json,"conflict_pairs":conflict_pairs,"rounds":rounds,"scripts":scripts,"summary":{"candidates_per_round":candidates.len(),"failed_rounds":failed,"duration_ms":duration,"leaks":leaks,"known_failure_observations":known_failure_observations}});
        if let Err(error) = write_json(&path, &document) {
            eprintln!("mx-test-isolation-proof: {error}");
            return 1;
        }
    }
    println!(
        "MX_ISOLATION_SUMMARY total={} failed_rounds={failed} concurrency={jobs} repeats={repeats} duration_ms={duration} leaks={leaks}",
        candidates.len()
    );
    i32::from(failed != 0 || leaks != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_is_unique_and_conflicts_are_symmetric() {
        let rows = manifest();
        assert_eq!(
            rows.len(),
            rows.iter()
                .map(|row| &row.path)
                .collect::<BTreeSet<_>>()
                .len()
        );
        assert!(resource_conflict("global", "none"));
        assert!(resource_conflict("watcher", "watcher"));
        assert!(!resource_conflict("none", "watcher"));
    }
    #[test]
    fn family_examples_are_stable() {
        assert_eq!(family("tests/mx-test-run.test.sh"), "pure-contract-unit");
        assert_eq!(
            family("tests/mx-backend-herdr-smoke.test.sh"),
            "real-herdr-gated"
        );
    }
}
