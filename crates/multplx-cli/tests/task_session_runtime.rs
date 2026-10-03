//! Managed hook provenance and read-only inspection with an inert provider.
use multplx_domain::lifecycle::{
    spawn::task_temp_path_for_record,
    subagent_model::{ArtifactKind, AssignmentRole, TaskRecord, write_meta},
};
use std::{fs, process::Command};
#[test]
fn exact_native_hook_owner_registers_and_nested_provider_cannot_replace_it() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    fs::create_dir(&state).unwrap();
    let mut task = TaskRecord::new(
        "worker".into(),
        AssignmentRole::Implementer,
        ArtifactKind::Implementation,
        false,
        "primary".into(),
        "primary".into(),
        root.path().to_str().unwrap().into(),
    );
    task.briefs[0].scope = "inert native hook fixture".into();
    task.runtime.provider = "herdr".into();
    task.runtime.endpoint = Some("mx-lab-fixture:w1:p2".into());
    let meta = write_meta(
        &format!("harness=codex\nworktree={}\n", root.path().display()),
        &task,
    )
    .unwrap();
    fs::write(state.join("worker.meta"), meta).unwrap();
    let tmp = task_temp_path_for_record(&task).unwrap();
    fs::create_dir_all(&tmp).unwrap();
    let attempt = task.attempt.as_ref().unwrap();
    let identity = serde_json::json!({"task":"worker","attempt":attempt,"endpoint":task.runtime.endpoint,"brief_revision":task.accepted_brief_revision,"nonce":"inert-fixture"});
    let provider = root.path().join("codex");
    fs::write(&provider,r#"import os,sys,json,pathlib,subprocess
mx,root,tmp,identity=sys.argv[1:]
root=pathlib.Path(root); tmp=pathlib.Path(tmp)
cmd=[mx,'supervision','mx-native-observe.sh','--provider','codex','--event','reconcile']
session='12345678-1234-1234-1234-123456789abc'
rollout=root/'rollout.jsonl'
payload={'session_id':session,'cwd':str(root),'transcript_path':str(rollout)}
def hook(value):
 p=subprocess.run(cmd,input=json.dumps(value),text=True,capture_output=True)
 return {'exit':p.returncode,'stderr':p.stderr}
if os.environ.get('NESTED'):
 (root/'nested-result.json').write_text(json.dumps(hook(payload)));sys.exit()
(tmp/'launch-started').write_text(identity+'\n'+str(os.getpid())+'\n')
rollout.write_text(json.dumps({'type':'session_meta','payload':{'id':session,'cwd':str(root)}})+'\n')
results=[hook(payload)]
results.append(hook({'session_id':session,'cwd':str(root)}))
results.append(hook(dict(payload,cwd=str(root/'state'))))
results.append(hook(dict(payload,session_id='22345678-1234-1234-1234-123456789abc')))
results.append(hook(dict(payload,transcript_path=7)))
e=dict(os.environ,NESTED='1'); subprocess.run([sys.executable,__file__,mx,str(root),str(tmp),identity],env=e,check=True)
(root/'results.json').write_text(json.dumps(results))
"#).unwrap();
    let binary = env!("CARGO_BIN_EXE_mx");
    let output = Command::new("python3")
        .arg(&provider)
        .args([
            binary,
            root.path().to_str().unwrap(),
            tmp.to_str().unwrap(),
            &identity.to_string(),
        ])
        .env("MX_ROOT_OVERRIDE", root.path())
        .env("MX_HOME", root.path())
        .env("MX_STATE_OVERRIDE", &state)
        .env("MX_REPORT_STATE_OVERRIDE", &state)
        .env("MX_TASK_ID", "worker")
        .env("MX_ATTEMPT_ID", &attempt.id)
        .env("MX_ATTEMPT_GENERATION", attempt.generation.to_string())
        .env("MX_BRIEF_REVISION", attempt.brief_revision.to_string())
        .env("MX_REAL_CODEX", &provider)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let results: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("results.json")).unwrap()).unwrap();
    assert_eq!(results[0]["exit"], 0, "{results}");
    assert_eq!(results[1]["exit"], 0, "{results}");
    for i in 2..5 {
        assert_eq!(results[i]["exit"], 0, "{results}");
        assert!(
            results[i]["stderr"]
                .as_str()
                .unwrap()
                .contains("identity unavailable")
        );
    }
    let nested: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("nested-result.json")).unwrap()).unwrap();
    assert_eq!(nested["exit"], 0);
    assert!(
        nested["stderr"]
            .as_str()
            .unwrap()
            .contains("exact launched provider process")
    );
    let inspect = |verb: &str| {
        Command::new(binary)
            .args(["task-session", verb, "worker"])
            .env("MX_STATE_OVERRIDE", &state)
            .env("MX_MULTICALL_EXPLICIT", "1")
            .output()
            .unwrap()
    };
    let observed = inspect("inspect");
    assert!(observed.status.success());
    let value: serde_json::Value = serde_json::from_slice(&observed.stdout).unwrap();
    assert_eq!(value["transcript_available"], true);
    assert_eq!(
        value["provider_session"]["session_id"],
        "12345678-1234-1234-1234-123456789abc"
    );
    fs::remove_file(root.path().join("rollout.jsonl")).unwrap();
    let unavailable = inspect("inspect");
    assert!(unavailable.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&unavailable.stdout).unwrap()["transcript_available"],
        false
    );
    fs::remove_file(state.join("worker.meta")).unwrap();
    let history = inspect("history");
    assert!(history.status.success());
    let value: serde_json::Value = serde_json::from_slice(&history.stdout).unwrap();
    assert_eq!(
        value["retained_sessions"][0]["current_attempt_bound"],
        false
    );
    assert_eq!(
        value["retained_sessions"][0]["live_execution_proven"],
        false
    );
    assert!(!inspect("inspect").status.success());
    fs::remove_dir_all(&tmp).unwrap();
    let help = Command::new(binary)
        .args(["task-session", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("multplx task-session"));
}
