//! Native bounded catch-up projection over the canonical system snapshot.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

pub(crate) fn run(args: &[String], source_root: &Path, home: &Path) -> (i32, String, String) {
    let mut json_output = false;
    let mut include_prs = false;
    let mut fields = String::new();
    let mut all = BTreeSet::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json_output = true,
            "--include-prs" => include_prs = true,
            "--fields" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return (2, String::new(), usage());
                };
                fields = value.clone();
            }
            value if value.starts_with("--fields=") => {
                fields = value.trim_start_matches("--fields=").to_owned()
            }
            "--all-in-flight" | "--all-decisions" | "--all-daemons" | "--all-landed"
            | "--all-reports" | "--all-queued" | "--all-recorded-prs" | "--all-unhealthy"
            | "--all-pr-repos" => {
                all.insert(args[index].clone());
            }
            "-h" | "--help" => return (0, usage(), String::new()),
            _ => return (2, String::new(), usage()),
        }
        index += 1;
    }
    let gate = Command::new(source_root.join("bin/mx-afk-return.sh"))
        .arg("guard")
        .output();
    if let Ok(gate) = gate
        && !gate.status.success()
    {
        return (
            gate.status.code().unwrap_or(1),
            String::from_utf8_lossy(&gate.stdout).into_owned(),
            String::from_utf8_lossy(&gate.stderr).into_owned(),
        );
    }
    let now = std::env::var("MX_STATUS_NOW").unwrap_or_else(|_| {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default()
    });
    let mut snapshot_command = Command::new(source_root.join("bin/mx-system-snapshot.sh"));
    snapshot_command.arg("--json").env("MX_SNAPSHOT_NOW", &now);
    if all.contains("--all-landed") {
        snapshot_command.env("MX_SNAPSHOT_DAEMON_LANDED_PER_HOME", "0");
    }
    if all.contains("--all-daemons") {
        snapshot_command.env("MX_SNAPSHOT_DAEMONS", "0");
    }
    let snapshot = snapshot_command.output();
    let Ok(snapshot) = snapshot else {
        return (
            1,
            String::new(),
            "mx-status-snapshot: canonical snapshot unavailable\n".into(),
        );
    };
    if !snapshot.status.success() {
        return (
            snapshot.status.code().unwrap_or(1),
            String::from_utf8_lossy(&snapshot.stdout).into_owned(),
            String::from_utf8_lossy(&snapshot.stderr).into_owned(),
        );
    }
    let Ok(root) = serde_json::from_slice::<Value>(&snapshot.stdout) else {
        return (
            1,
            String::new(),
            "mx-status-snapshot: invalid canonical snapshot\n".into(),
        );
    };
    let bound = |name: &str, default: usize| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(default)
    };
    let take = |mut rows: Vec<Value>, flag: &str, limit: usize| {
        if !all.contains(flag) {
            rows.truncate(limit);
        }
        rows
    };
    let backlog = root
        .pointer("/backlog/records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tasks = root
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let daemons = root
        .pointer("/daemon_current/records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let working_ids = tasks
        .iter()
        .filter(|task| {
            task.get("kind").and_then(Value::as_str) != Some("daemon")
                && task.pointer("/current_state/state").and_then(Value::as_str) == Some("working")
        })
        .filter_map(|task| task.get("id").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let mut in_flight = tasks
        .iter()
        .filter(|task| task.get("kind").and_then(Value::as_str) != Some("daemon"))
        .filter(|task| task.pointer("/backlog/current_role").and_then(Value::as_str) != Some("program"))
        .filter(|task| {
            task.pointer("/backlog/current_role").and_then(Value::as_str) != Some("held")
                || task.pointer("/current_state/state").and_then(Value::as_str) == Some("working")
        })
        .map(|task| {
            let detail = nonnull_str(task.pointer("/current_state/detail"));
            let doing = if detail.is_empty() {
                nonnull_str(task.pointer("/hints/last_event_text"))
            } else {
                detail
            };
            json!({"id":task["id"],"kind":task["kind"],"state":task["current_state"]["state"],"doing":truncate(&doing,90)})
        })
        .collect::<Vec<_>>();
    for daemon in &daemons {
        if daemon_status_state(daemon) == "active_child_work" {
            in_flight.push(json!({"id":daemon["id"],"kind":"daemon","state":"active_child_work","doing":truncate(&daemon_active_summary(daemon),90)}));
        }
    }

    let mut decisions = backlog
        .iter()
        .filter(|row| row.get("structured").and_then(Value::as_bool) == Some(true))
        .filter(|row| row.get("maintainer_actionable").and_then(Value::as_bool) == Some(true))
        .map(|row| json!({"id":row["id"],"key":row["id"],"verb":"maintainer-hold","summary":truncate(&format!("{}: {}",nonnull_str(row.get("title")),nonnull_str(row.get("hold_reason"))),90),"owner":"(main)"}))
        .collect::<Vec<_>>();
    for daemon in &daemons {
        for row in array(daemon.get("decisions_open")) {
            if row.get("source").and_then(Value::as_str) == Some("backlog")
                && row.get("verb").and_then(Value::as_str) == Some("maintainer-hold")
            {
                let id = nonnull_str(row.get("id"));
                decisions.push(json!({"id":format!("{}/{}",nonnull_str(daemon.get("id")),id),"key":row.get("key").cloned().unwrap_or_else(||Value::String(id.clone())),"verb":"maintainer-hold","summary":truncate(&format!("{}: {}",value_or(row.get("summary"),&id),value_or(row.get("reason"),"maintainer decision pending")),90),"owner":daemon["id"]}));
            }
        }
    }
    let mut gates = Vec::new();
    if root
        .pointer("/main_inventory/valid")
        .and_then(Value::as_bool)
        == Some(false)
    {
        gates.push(json!({"id":"(main-inventory)","title":truncate(&value_or(root.pointer("/main_inventory/reason"),"main inventory invalid"),60),"blocked_by":"-","reason":"main inventory","owner":"(main)"}));
    }
    for row in &backlog {
        let queued = row.get("state").and_then(Value::as_str) == Some("queued");
        let held = row.get("state").and_then(Value::as_str) == Some("in_flight")
            && row.get("current_role").and_then(Value::as_str) == Some("held")
            && !working_ids.contains(nonnull_str(row.get("id")).as_str());
        let excerpt = nonnull_str(row.get("body_excerpt")).to_ascii_uppercase();
        let superseded = ["SUPERSEDED", "NOT REQUIRED", "NOT-REQUIRED", "DEFERRED"]
            .iter()
            .any(|needle| excerpt.contains(needle));
        if row.get("structured").and_then(Value::as_bool) == Some(true)
            && (queued || held)
            && row.get("maintainer_actionable").and_then(Value::as_bool) != Some(true)
            && (all.contains("--all-queued") || !superseded)
        {
            gates.push(gate_row(row, Value::String("(main)".into())));
        }
    }
    for daemon in &daemons {
        if daemon
            .pointer("/provenance/selected")
            .and_then(Value::as_str)
            == Some("structured-home")
        {
            for row in array(daemon.get("queued")) {
                if row.get("maintainer_actionable").and_then(Value::as_bool) != Some(true) {
                    gates.push(gate_row(row, daemon["id"].clone()));
                }
            }
        }
    }
    let mut landed=backlog.iter().filter(|row|row.get("state").and_then(Value::as_str)==Some("done")&&row.get("kind").and_then(Value::as_str)!=Some("maintainer")).map(|row|json!({"id":row["id"],"what":row["title"],"artifact":row.get("pr_url").or_else(||row.get("report_path")).or_else(||row.get("local_note")).cloned().unwrap_or(Value::String("-".into())),"owner":"(main)"})).collect::<Vec<_>>();
    for row in root
        .pointer("/daemon_landed/records")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        landed.push(json!({"id":row["id"],"what":row.get("title").cloned().unwrap_or_else(||row["id"].clone()),"artifact":row.get("pr_url").or_else(||row.get("report_path")).or_else(||row.get("local_note")).cloned().unwrap_or(Value::String("-".into())),"owner":row.get("home_id").or_else(||row.get("home")).cloned().unwrap_or(Value::String("daemon".into()))}));
    }
    let mut daemon_rows = Vec::new();
    if root
        .pointer("/daemon_current/registry/available")
        .and_then(Value::as_bool)
        == Some(false)
    {
        let reason = value_or(
            root.pointer("/daemon_current/registry/reason"),
            "Registered daemon table unavailable",
        );
        daemon_rows.push(json!({"id":"(registry)","state":"unknown","doing":reason,"provenance":value_or(root.pointer("/daemon_current/registry/provenance"),"registered-table"),"freshness":value_or(root.pointer("/daemon_current/registry/freshness/status"),"unavailable"),"age_seconds":Value::Null,"contradiction":false,"reason":reason}));
    }
    for daemon in &daemons {
        let state = daemon_status_state(daemon);
        daemon_rows.push(json!({"id":daemon["id"],"state":state,"doing":truncate(&daemon_doing(daemon,&state),120),"provenance":daemon.pointer("/provenance/selected").cloned().unwrap_or(Value::String("unknown".into())),"freshness":daemon.pointer("/freshness/status").cloned().unwrap_or(Value::String("unknown".into())),"age_seconds":daemon.pointer("/freshness/age_seconds").cloned().unwrap_or(Value::Null),"contradiction":daemon.get("contradiction").cloned().unwrap_or(Value::Bool(false)),"reason":value_or(daemon.pointer("/current/reason"),"-")}));
    }
    let reports = root
        .get("scout_reports")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let recorded: Vec<Value> = tasks
        .iter()
        .filter(|task| task.get("kind").and_then(Value::as_str) != Some("daemon"))
        .filter(|task| task.pointer("/pr/source").and_then(Value::as_str) == Some("meta"))
        .filter_map(|task| {
            task.pointer("/pr/url")
                .and_then(Value::as_str)
                .map(|url| json!({"id":task["id"],"url":url}))
        })
        .collect();
    let mut unhealthy: Vec<Value> = tasks.iter().filter(|task|task.pointer("/endpoint/exists").and_then(Value::as_bool)==Some(false)||task.pointer("/endpoint/agent_alive").and_then(Value::as_str)==Some("dead")).map(|task|json!({"id":task["id"],"backend":task["backend"],"target":task.pointer("/endpoint/target").cloned().unwrap_or(Value::String("-".into())),"exists":task["endpoint"]["exists"],"agent":task["endpoint"]["agent_alive"]})).collect();
    for daemon in &daemons {
        for endpoint in array(daemon.get("endpoints")) {
            if endpoint
                .pointer("/endpoint/exists")
                .and_then(Value::as_bool)
                == Some(false)
                || endpoint
                    .pointer("/endpoint/agent_alive")
                    .and_then(Value::as_str)
                    == Some("dead")
            {
                unhealthy.push(json!({"id":format!("{}/{}",nonnull_str(daemon.get("id")),nonnull_str(endpoint.get("id"))),"backend":"daemon-home","target":value_or(endpoint.pointer("/endpoint/target"),"-"),"exists":endpoint["endpoint"]["exists"],"agent":endpoint["endpoint"]["agent_alive"]}));
            }
        }
    }
    let totals = [
        (
            "in_flight",
            in_flight.len(),
            "--all-in-flight",
            "MX_STATUS_IN_FLIGHT",
            20usize,
        ),
        (
            "decisions_open",
            decisions.len(),
            "--all-decisions",
            "MX_STATUS_DECISIONS",
            20,
        ),
        ("gates", gates.len(), "--all-queued", "MX_STATUS_GATES", 20),
        (
            "reports",
            reports.len(),
            "--all-reports",
            "MX_STATUS_REPORTS",
            20,
        ),
        (
            "recorded_prs",
            recorded.len(),
            "--all-recorded-prs",
            "MX_STATUS_RECORDED_PRS",
            20,
        ),
        (
            "unhealthy_endpoints",
            unhealthy.len(),
            "--all-unhealthy",
            "MX_STATUS_UNHEALTHY",
            20,
        ),
    ];
    let home_label = home
        .components()
        .rev()
        .take(2)
        .map(|v| v.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("/");
    let (landed, landed_available, landed_home_caps) = balanced_landed(
        landed,
        all.contains("--all-landed"),
        bound("MX_STATUS_LANDED", 6),
        bound("MX_STATUS_LANDED_PER_HOME", bound("MX_STATUS_LANDED", 6)),
    );
    let daemon_total = daemon_rows.len();
    let mut model = json!({"schema":"mx-catchup.v1","home":home_label,"generated":now,"prs":if include_prs{"checked"}else{"not_requested (run: /catchup include PRs)"},"in_flight":take(in_flight,"--all-in-flight",bound("MX_STATUS_IN_FLIGHT",20)),"daemons":take(daemon_rows,"--all-daemons",bound("MX_STATUS_DAEMONS",20)),"decisions_open":take(decisions,"--all-decisions",bound("MX_STATUS_DECISIONS",20)),"landed":landed,"gates":take(gates,"--all-queued",bound("MX_STATUS_GATES",20)),"reports":take(reports,"--all-reports",bound("MX_STATUS_REPORTS",20)),"recorded_prs":take(recorded,"--all-recorded-prs",bound("MX_STATUS_RECORDED_PRS",20)),"omitted":[{"surface":"live PR discovery + checks","reveal":"--include-prs"}]});
    let selected_fields = fields.split(',').map(str::trim).collect::<BTreeSet<_>>();
    let omitted = model["omitted"].as_array_mut().unwrap();
    for (field, surface, reveal) in [
        ("bodies", "backlog item bodies", "--fields bodies"),
        ("paths", "task paths", "--fields paths"),
        ("actions", "watch/steer actions", "--fields actions"),
        ("endpoints", "healthy endpoint detail", "--fields endpoints"),
    ] {
        if !selected_fields.contains(field) {
            omitted.push(json!({"surface":surface,"reveal":reveal}));
        }
    }
    if !all.contains("--all-reports") {
        omitted.push(json!({"surface":"full scout-report inventory","reveal":"--all-reports"}));
    }
    if !all.contains("--all-queued") {
        omitted.push(json!({"surface":"superseded queued items","reveal":"--all-queued"}));
    }
    let daemon_limit = bound("MX_STATUS_DAEMONS", 20);
    if !all.contains("--all-daemons") && daemon_total > daemon_limit {
        omitted.push(json!({"surface":format!("daemons showing {daemon_limit} of {daemon_total}"),"reveal":"--all-daemons"}));
    }
    let unreadable = root
        .pointer("/daemon_landed/unreadable")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if unreadable > 0 {
        omitted.push(json!({"surface":format!("daemon home(s) with unreadable backlog: {unreadable}"),"reveal":"inspect the listed daemon home backlogs"}));
    }
    let orphaned = root
        .pointer("/main_inventory/orphan_in_flight")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if orphaned > 0 {
        omitted.push(json!({"surface":format!("main in-flight backlog item(s) have no child metadata: {orphaned}"),"reveal":"inspect main data/backlog.md In flight vs state/*.meta"}));
    }
    let unstructured = root
        .pointer("/main_inventory/unstructured_current_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if unstructured > 0 {
        omitted.push(json!({"surface":format!("main unstructured current backlog row(s): {unstructured}"),"reveal":"inspect main data/backlog.md In flight and Queued free-form rows"}));
    }
    let daemon_truncated = root
        .pointer("/daemon_current/truncated")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if daemon_truncated > 0 {
        omitted.push(json!({"surface":format!("registered daemons omitted by snapshot bound: {daemon_truncated}"),"reveal":"raise MX_SNAPSHOT_DAEMONS"}));
    }
    if root
        .pointer("/daemon_current/registry/input_truncated")
        .and_then(Value::as_bool)
        == Some(true)
    {
        omitted.push(json!({"surface":"daemon registry input truncated by bounded read","reveal":"raise MX_SNAPSHOT_REGISTRY_LINES or MX_SNAPSHOT_REGISTRY_BYTES"}));
    }
    if root
        .pointer("/daemon_current/registry/records_truncated")
        .and_then(Value::as_bool)
        == Some(true)
    {
        omitted.push(json!({"surface":"daemon registry records omitted by bounded read","reveal":"raise MX_SNAPSHOT_REGISTRY_RECORDS"}));
    }
    if root
        .pointer("/daemon_current/registry/available")
        .and_then(Value::as_bool)
        == Some(false)
    {
        omitted.push(json!({"surface":format!("daemon registry unavailable: {}",value_or(root.pointer("/daemon_current/registry/reason"),"read failed")),"reveal":"inspect data/daemons.md"}));
    }
    let parent_truncated = daemons
        .iter()
        .filter(|row| {
            row.pointer("/parent_event/activity_scan/input_truncated")
                .and_then(Value::as_bool)
                == Some(true)
                || row
                    .pointer("/parent_event/activity_scan/retained_truncated")
                    .and_then(Value::as_bool)
                    == Some(true)
        })
        .count();
    if parent_truncated > 0 {
        omitted.push(json!({"surface":format!("daemon parent activity evidence truncated for {parent_truncated} record(s)"),"reveal":"raise MX_SNAPSHOT_PARENT_ACTIVITY_LINES, MX_SNAPSHOT_PARENT_ACTIVITY_BYTES, or MX_SNAPSHOT_PARENT_ACTIVITIES"}));
    }
    let parent_unavailable = daemons
        .iter()
        .filter(|row| {
            row.pointer("/parent_event/activity_scan/available")
                .and_then(Value::as_bool)
                == Some(false)
        })
        .count();
    if parent_unavailable > 0 {
        omitted.push(json!({"surface":format!("daemon parent activity evidence unavailable for {parent_unavailable} record(s)"),"reveal":"inspect the parent status logs"}));
    }
    if !unhealthy.is_empty() {
        model["unhealthy_endpoints"] = Value::Array(take(
            unhealthy,
            "--all-unhealthy",
            bound("MX_STATUS_UNHEALTHY", 20),
        ));
    }
    for (surface, total, flag, environment, default) in totals {
        let limit = bound(environment, default);
        if !all.contains(flag) && total > limit {
            model["omitted"].as_array_mut().unwrap().push(
                json!({"surface":format!("{surface} showing {limit} of {total}"),"reveal":flag}),
            );
        }
    }
    if !all.contains("--all-landed")
        && landed_available > model["landed"].as_array().map_or(0, Vec::len)
    {
        let shown = model["landed"].as_array().map_or(0, Vec::len);
        model["omitted"].as_array_mut().unwrap().push(
            json!({"surface":format!("landed showing {shown} of {landed_available}"),"reveal":"--all-landed"}),
        );
    }
    if !all.contains("--all-landed") && landed_home_caps > 0 {
        model["omitted"].as_array_mut().unwrap().push(
            json!({"surface":format!("landed per-home capped for {landed_home_caps} home(s)"),"reveal":"--all-landed"}),
        );
    }
    if !all.contains("--all-landed")
        && root
            .pointer("/daemon_landed/truncated")
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.is_empty())
    {
        let count = root
            .pointer("/daemon_landed/truncated")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        model["omitted"].as_array_mut().unwrap().push(json!({"surface":format!("daemon home Done capped at the snapshot layer for {count} home(s)"),"reveal":"--all-landed"}));
    }
    if include_prs {
        let (rows, failures, repo_total, repo_shown, capped) =
            live_prs(&model, all.contains("--all-pr-repos"));
        model["candidate_prs"] = Value::Array(rows);
        model["prs"] = Value::String(if failures > 0 {
            format!("unavailable ({failures} repo(s))")
        } else if capped > 0 {
            format!(
                "checked ({repo_shown} repos; {} shown, at least {} open; capped in {capped} repo(s))",
                model["candidate_prs"].as_array().map_or(0, Vec::len),
                model["candidate_prs"].as_array().map_or(0, Vec::len) + capped
            )
        } else {
            format!(
                "checked ({} open)",
                model["candidate_prs"].as_array().map_or(0, Vec::len)
            )
        });
        model["omitted"].as_array_mut().unwrap().retain(|v| {
            v.get("surface").and_then(Value::as_str) != Some("live PR discovery + checks")
        });
        if repo_total > repo_shown {
            model["omitted"].as_array_mut().unwrap().push(json!({"surface":format!("PR repositories showing {repo_shown} of {repo_total}"),"reveal":"--all-pr-repos"}));
        }
        if capped > 0 {
            let shown = model["candidate_prs"].as_array().map_or(0, Vec::len);
            model["omitted"].as_array_mut().unwrap().push(json!({"surface":format!("candidate_prs showing {shown} of at least {}; capped in {capped} repo(s)",shown+capped),"reveal":"raise MX_STATUS_PR_LIMIT"}));
        }
    }
    for field in fields.split(',').map(str::trim) {
        match field{"paths"=>model["paths"]=Value::Array(tasks.iter().map(|t|json!({"id":t["id"],"worktree":t.pointer("/paths/worktree/path").cloned().unwrap_or(Value::Null)})).collect()),"endpoints"=>model["endpoints"]=Value::Array(tasks.iter().map(|t|json!({"id":t["id"],"backend":t["backend"],"target":t.pointer("/endpoint/target").cloned().unwrap_or(Value::Null)})).collect()),_=>{}}
    }
    if std::env::var("MX_STATUS_TEST_FAIL_PHASE").as_deref() == Ok("model") {
        return (
            1,
            String::new(),
            "mx-status-snapshot: projection failed\n".into(),
        );
    }
    let encoded = serde_json::to_string_pretty(&model).unwrap();
    if json_output {
        (0, format!("{encoded}\n"), String::new())
    } else if std::env::var("MX_STATUS_TEST_FAIL_PHASE").as_deref() == Ok("toon") {
        (
            1,
            String::new(),
            "mx-status-snapshot: TOON rendering failed\n".into(),
        )
    } else {
        (0, toon(&model), String::new())
    }
}

fn array(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn nonnull_str(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn value_or(value: Option<&Value>, fallback: &str) -> String {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

fn truncate(value: &str, maximum: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(maximum).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}

fn daemon_status_state(daemon: &Value) -> String {
    let current = value_or(daemon.pointer("/current/state"), "unknown");
    if current != "maintainer_decision" {
        return current;
    }
    let maintainer_holds = array(daemon.get("decisions_open")).iter().any(|row| {
        row.get("source").and_then(Value::as_str) == Some("backlog")
            && row.get("verb").and_then(Value::as_str) == Some("maintainer-hold")
    });
    if maintainer_holds {
        "maintainer_decision".into()
    } else if !array(daemon.get("active_children")).is_empty() {
        "active_child_work".into()
    } else if array(daemon.get("holds"))
        .iter()
        .any(|row| row.get("source").and_then(Value::as_str) == Some("backlog"))
    {
        "externally_held".into()
    } else {
        "unknown".into()
    }
}

fn daemon_active_summary(daemon: &Value) -> String {
    array(daemon.get("active_children"))
        .iter()
        .map(|row| {
            format!(
                "{}: {}",
                nonnull_str(row.get("id")),
                value_or(row.get("doing").or_else(|| row.get("state")), "unknown")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn daemon_doing(daemon: &Value, state: &str) -> String {
    match state {
        "active_child_work" => daemon_active_summary(daemon),
        "maintainer_decision" => array(daemon.get("decisions_open"))
            .iter()
            .filter(|row| {
                row.get("source").and_then(Value::as_str) == Some("backlog")
                    && row.get("verb").and_then(Value::as_str) == Some("maintainer-hold")
            })
            .map(|row| value_or(row.get("summary"), "maintainer decision pending"))
            .collect::<Vec<_>>()
            .join("; "),
        "externally_held" => {
            let holds = array(daemon.get("holds"));
            let relevant = if daemon.pointer("/current/state").and_then(Value::as_str)
                == Some("maintainer_decision")
            {
                holds
                    .iter()
                    .filter(|row| row.get("source").and_then(Value::as_str) == Some("backlog"))
                    .collect::<Vec<_>>()
            } else {
                holds.iter().collect::<Vec<_>>()
            };
            relevant
                .into_iter()
                .map(|row| {
                    format!(
                        "{}: {}",
                        nonnull_str(row.get("id")),
                        value_or(row.get("reason"), "held")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        }
        "no_active_work" => "No active child work".into(),
        _ => value_or(
            daemon.pointer("/current/reason"),
            "Current home state unavailable",
        ),
    }
}

fn gate_row(row: &Value, owner: Value) -> Value {
    let blocked_by = array(row.get("unresolved_blocker_ids"))
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(",");
    json!({
        "id": row["id"],
        "title": truncate(&nonnull_str(row.get("title")),60),
        "blocked_by": truncate(if blocked_by.is_empty() { "-" } else { &blocked_by },120),
        "reason": truncate(&value_or(row.get("hold_reason").or_else(||row.get("blocked_reason")),"-"),40),
        "owner": owner,
    })
}

fn balanced_landed(
    rows: Vec<Value>,
    all: bool,
    limit: usize,
    per_home: usize,
) -> (Vec<Value>, usize, usize) {
    let total = rows.len();
    if all {
        return (rows, total, 0);
    }
    let mut groups = BTreeMap::<String, Vec<Value>>::new();
    for row in rows {
        groups
            .entry(row["owner"].as_str().unwrap_or("daemon").to_owned())
            .or_default()
            .push(row);
    }
    let capped = groups.values().filter(|rows| rows.len() > per_home).count();
    for rows in groups.values_mut() {
        rows.truncate(per_home);
    }
    let available = groups.values().map(Vec::len).sum();
    let mut output = Vec::new();
    let max = groups.values().map(Vec::len).max().unwrap_or(0);
    for index in 0..max {
        for rows in groups.values() {
            if let Some(row) = rows.get(index) {
                output.push(row.clone());
                if output.len() == limit {
                    return (output, available, capped);
                }
            }
        }
    }
    (output, available, capped)
}

fn live_prs(model: &Value, all_repos: bool) -> (Vec<Value>, usize, usize, usize, usize) {
    let mut repos = BTreeSet::new();
    for row in model["recorded_prs"].as_array().into_iter().flatten() {
        if let Some(url) = row["url"].as_str()
            && let Some(tail) = url.split("github.com/").nth(1)
        {
            let parts = tail.split('/').take(2).collect::<Vec<_>>();
            if parts.len() == 2 {
                repos.insert(format!(
                    "{}/{}",
                    parts[0],
                    parts[1].trim_end_matches(".git")
                ));
            }
        }
    }
    let mut rows = Vec::new();
    let mut failures = 0;
    let repo_limit = std::env::var("MX_STATUS_PR_REPOS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v: &usize| *v > 0)
        .unwrap_or(10);
    let row_limit = std::env::var("MX_STATUS_PR_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v: &usize| *v > 0)
        .unwrap_or(20);
    let timeout = std::env::var("MX_STATUS_PR_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v: &u64| *v > 0)
        .unwrap_or(20);
    let repo_total = repos.len();
    let repo_shown = if all_repos {
        repo_total
    } else {
        repo_total.min(repo_limit)
    };
    let mut capped = 0;
    for repo in repos.into_iter().take(repo_shown) {
        let child = Command::new("gh")
            .args([
                "pr",
                "list",
                "--repo",
                &repo,
                "--state",
                "open",
                "--limit",
                &(row_limit + 1).to_string(),
                "--json",
                "number,title,url,headRefName,reviewDecision,mergeable,statusCheckRollup",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();
        let result = child.ok().and_then(|mut child| {
            let start = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return child.wait_with_output().ok(),
                    Ok(None) if start.elapsed() >= Duration::from_secs(timeout) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return None;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                    Err(_) => return None,
                }
            }
        });
        match result {
            Some(result) if result.status.success() => {
                if let Ok(found) = serde_json::from_slice::<Vec<Value>>(&result.stdout) {
                    if found.len() > row_limit {
                        capped += 1;
                    }
                    for row in found.into_iter().take(row_limit) {
                        let checks =
                            if row["statusCheckRollup"]
                                .as_array()
                                .is_none_or(Vec::is_empty)
                            {
                                "none"
                            } else if row["statusCheckRollup"].as_array().unwrap().iter().any(
                                |check| {
                                    matches!(
                                        check.get("conclusion").and_then(Value::as_str),
                                        Some(
                                            "FAILURE"
                                                | "ERROR"
                                                | "TIMED_OUT"
                                                | "CANCELLED"
                                                | "ACTION_REQUIRED"
                                        )
                                    )
                                },
                            ) {
                                "failing"
                            } else if row["statusCheckRollup"].as_array().unwrap().iter().any(
                                |check| {
                                    check.get("status").and_then(Value::as_str) != Some("COMPLETED")
                                        && check.get("state").and_then(Value::as_str)
                                            != Some("SUCCESS")
                                },
                            ) {
                                "pending"
                            } else {
                                "passing"
                            };
                        let branch = row["headRefName"].as_str().unwrap_or("");
                        rows.push(json!({
                            "num": row["number"].to_string(), "repo": repo,
                            "task": branch.strip_prefix("mx/").unwrap_or("-"),
                            "url": row.get("url").cloned().unwrap_or(Value::String("-".into())),
                            "review": row.get("reviewDecision").and_then(Value::as_str).filter(|v| !v.is_empty()).unwrap_or("none"),
                            "mergeable": row.get("mergeable").and_then(Value::as_str).unwrap_or("UNKNOWN"),
                            "checks": checks
                        }));
                    }
                }
            }
            _ => failures += 1,
        }
    }
    (rows, failures, repo_total, repo_shown, capped)
}
fn toon(value: &Value) -> String {
    let mut out = String::new();
    for (key, value) in value.as_object().into_iter().flatten() {
        if let Some(rows) = value.as_array() {
            if rows.is_empty() {
                out.push_str(&format!("{key}: []\n"));
            } else {
                let keys = rows[0]
                    .as_object()
                    .map(|row| row.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                out.push_str(&format!(
                    "{key}[{}]{{{}}}:\n",
                    rows.len(),
                    keys.iter()
                        .map(|v| toon_quote(v))
                        .collect::<Vec<_>>()
                        .join(",")
                ));
                for row in rows {
                    out.push_str("  ");
                    out.push_str(
                        &keys
                            .iter()
                            .map(|field| toon_scalar(&row[field]))
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                    out.push('\n');
                }
            }
        } else {
            out.push_str(&format!("{key}: {}\n", toon_scalar(value)));
        }
    }
    out
}
fn toon_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => toon_quote(v),
        other => toon_quote(&other.to_string()),
    }
}
fn toon_quote(value: &str) -> String {
    let reserved = value.is_empty()
        || value.starts_with(char::is_whitespace)
        || value.ends_with(char::is_whitespace)
        || value.starts_with('-')
        || matches!(value, "true" | "false" | "null")
        || value.parse::<f64>().is_ok()
        || value
            .chars()
            .any(|c| matches!(c, ':' | '"' | '\\' | '[' | ']' | '{' | '}' | ',') || c.is_control());
    if reserved {
        serde_json::to_string(value).unwrap()
    } else {
        value.to_owned()
    }
}
fn usage() -> String {
    "usage: mx-status-snapshot.sh [--json] [--include-prs] [--fields <list>] [--all-in-flight] [--all-decisions] [--all-daemons] [--all-landed] [--all-reports] [--all-queued] [--all-recorded-prs] [--all-unhealthy] [--all-pr-repos]\n".into()
}
