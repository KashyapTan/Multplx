//! Real Git and CLI lifecycle contracts with a deterministic cmux mock and
//! failing sentinels for unrelated endpoint/provider tools.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    fake: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let home = root.join("home");
        let project = root.join("project");
        let fake = root.join("fake");
        for path in [
            home.join("state"),
            home.join("config"),
            home.join("data/task"),
            home.join("tmp"),
            project.clone(),
            fake.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(
            home.join("data/task/brief.md"),
            "Inspect this fixture repository.\n",
        )
        .unwrap();
        fs::write(home.join("config/actor-harness"), "codex\n").unwrap();
        for tool in ["tmux", "herdr", "treehouse", "curl"] {
            let path = fake.join(tool);
            fs::write(
                &path,
                "#!/bin/sh\nprintf '%s\\n' \"$0 $*\" >> \"$MX_SENTINEL_LOG\"\nexit 99\n",
            )
            .unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let fixture = Self {
            _temp: temp,
            home,
            project,
            fake,
        };
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ]);
        fixture
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.project)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn spawn(&self, options: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mx"));
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("MX_") {
                command.env_remove(key);
            }
        }
        command
            .current_dir(&self.home)
            .env("MX_HOME", &self.home)
            .env("MX_ROOT_OVERRIDE", &self.home)
            .env(
                "MX_RUST_SOURCE_ROOT",
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
            )
            .env("MX_HEADROOM_SKIP_QUEUE", "1")
            .env("MX_HEADROOM_CPU_COUNT", "8")
            .env("MX_HEADROOM_LOAD1", "0")
            .env("MX_HEADROOM_MEM_AVAILABLE_BYTES", "17179869184")
            .env("MX_HEADROOM_API_CAPACITY", "8")
            .env("MX_HEADROOM_IN_USE", "0")
            .env("TMPDIR", self.home.join("tmp"))
            .env("MX_SENTINEL_LOG", self.home.join("sentinel.log"))
            .env(
                "PATH",
                format!("{}:{}", self.fake.display(), std::env::var("PATH").unwrap()),
            )
            .args(["spawn", "task"])
            .arg(&self.project)
            .args(["--backend", "tmux"])
            .args(options);
        command
    }

    fn refused(&self, output: Output, message: &str) {
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !self.home.join("sentinel.log").exists(),
            "preflight invoked endpoint/provider"
        );
        assert!(!self.home.join("state/task.meta").exists());
        assert_eq!(
            self.git(&["worktree", "list", "--porcelain"])
                .matches("worktree ")
                .count(),
            1
        );
    }
}

#[test]
fn single_checkout_refuses_dirty_detached_and_already_reserved_sources_before_allocation() {
    for kind in ["dirty", "detached", "reserved", "missing-grant", "scout"] {
        let f = Fixture::new();
        let before = f.git(&["rev-parse", "HEAD"]);
        let expected = match kind {
            "dirty" => {
                fs::write(f.project.join("human-work"), "retain me").unwrap();
                "requires a clean checkout"
            }
            "detached" => {
                f.git(&["checkout", "--detach", "-q"]);
                "requires an attached base branch"
            }
            "reserved" => {
                let digest =
                    multplx_domain::maintainer_override::sha256_text(f.project.to_str().unwrap());
                fs::write(
                    f.home.join(format!("state/.single-checkout-{digest}.json")),
                    "existing receipt",
                )
                .unwrap();
                "already has a single-checkout reservation"
            }
            "scout" => "supported only for one delivery task",
            _ => "request identity is duplicated, missing, or misplaced: missing-grant",
        };
        let mut options = vec!["--single-checkout", "missing-grant"];
        if kind == "scout" {
            options.push("--scout");
        }
        f.refused(f.spawn(&options).output().unwrap(), expected);
        assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
        assert!(!f.home.join("state/.spawn-task.intent").exists());
        if kind == "dirty" {
            assert_eq!(
                fs::read_to_string(f.project.join("human-work")).unwrap(),
                "retain me"
            );
        }
    }
}

#[test]
fn durable_launch_intent_retries_once_with_the_original_base_and_allocation() {
    let f = Fixture::new();
    let before = f.git(&["rev-parse", "HEAD"]);
    f.refused(
        f.spawn(&[])
            .env("MX_SPAWN_FAULT", "after-intent")
            .output()
            .unwrap(),
        "interruption after durable launch intent",
    );
    let intent_path = f.home.join("state/.spawn-task.intent");
    let intent = fs::read(&intent_path).unwrap();
    let binding: serde_json::Value = serde_json::from_slice(&intent).unwrap();
    assert!(String::from_utf8_lossy(&intent).contains(&before));
    assert!(binding["attempt"]["id"].is_string());
    f.git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--allow-empty",
        "-qm",
        "source moved",
    ]);
    let moved = f.git(&["rev-parse", "HEAD"]);
    let retry = f.spawn(&[]).output().unwrap();
    assert_eq!(retry.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&retry.stderr).contains("backend command failed"));
    let action_path = f.home.join("state/.spawn-actions/task.json");
    let action: serde_json::Value =
        serde_json::from_slice(&fs::read(&action_path).expect("action receipt")).unwrap();
    assert_eq!(action["binding"]["project"]["starting_revision"], before);
    assert_eq!(action["stage"], "failed");
    let allocation = action["binding"]["allocation"]["allocation_id"]
        .as_str()
        .expect("allocation")
        .to_owned();
    let worktrees = f
        .git(&["worktree", "list", "--porcelain"])
        .matches("worktree ")
        .count();
    let repeated = f.spawn(&[]).output().unwrap();
    assert_eq!(repeated.status.code(), Some(1));
    let repeated_action: serde_json::Value =
        serde_json::from_slice(&fs::read(action_path).expect("repeated action")).unwrap();
    assert_eq!(
        repeated_action["binding"]["allocation"]["allocation_id"],
        allocation
    );
    assert_eq!(
        f.git(&["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        worktrees
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), moved);
}

#[test]
fn invalid_launch_configuration_refuses_without_an_allocation_or_intent() {
    let f = Fixture::new();
    fs::write(f.home.join("config/subagent-dispatch.json"), "{}\n").unwrap();
    f.refused(f.spawn(&[]).output().unwrap(), "pass an explicit harness");
    assert!(!f.home.join("state/.spawn-task.intent").exists());
    f.refused(
        f.spawn(&["--replace-attempt"]).output().unwrap(),
        "requires current attempt id",
    );
    f.refused(
        f.spawn(&["--single-checkout"]).output().unwrap(),
        "requires",
    );
    f.refused(
        f.spawn(&["--resource", "session=2"]).output().unwrap(),
        "invalid or reserved",
    );
    f.refused(
        f.spawn(&["--resource", "gpu=0"]).output().unwrap(),
        "invalid or reserved",
    );
    f.refused(
        f.spawn(&["--resource"]).output().unwrap(),
        "requires NAME=UNITS",
    );
    assert!(!f.home.join("state/.spawn-task.intent").exists());
}

#[test]
fn invalid_filesystem_and_byte_inputs_cannot_publish_or_teardown_an_execution() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let f = Fixture::new();
    let blocked = f.home.join("blocked-state");
    fs::write(&blocked, "private state must survive").unwrap();
    f.refused(
        f.spawn(&[])
            .env("MX_STATE_OVERRIDE", &blocked)
            .output()
            .unwrap(),
        "cannot create state directory",
    );
    assert_eq!(
        fs::read_to_string(blocked).unwrap(),
        "private state must survive"
    );
    let invalid = OsString::from_vec(vec![0xff]);
    let output = Command::new(env!("CARGO_BIN_EXE_mx"))
        .args(["spawn", "task=project"])
        .arg(&invalid)
        .env("MX_HOME", &f.home)
        .output()
        .unwrap();
    f.refused(output, "spawn argument is not valid UTF-8");
    for args in [
        vec![
            OsString::from("teardown"),
            invalid.clone(),
            "--override".into(),
            "request".into(),
        ],
        vec![
            OsString::from("teardown"),
            "task".into(),
            "--override".into(),
            invalid,
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_mx"))
            .args(args)
            .env("MX_HOME", &f.home)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("not valid UTF-8"));
    }
    assert!(!f.home.join("state/.spawn-task.intent").exists());
    assert!(!f.home.join("state/task.meta").exists());
}

#[test]
fn native_cmux_launch_binds_exact_git_allocation_and_retains_it_after_endpoint_failure() {
    for failure in ["none", "after-endpoint", "after-metadata"] {
        let fail_after_endpoint = failure == "after-endpoint";
        let fail_after_metadata = failure == "after-metadata";
        let f = Fixture::new();
        let cmux = f.fake.join("cmux");
        fs::write(&cmux, r#"#!/bin/sh
set -eu
case "$1" in
  version) echo 'cmux 0.64.17 (97) [abcdef1]' ;;
  ping) echo PONG ;;
  workspace)
    if [ -f "$MX_CMUX_FIXTURE/title" ]; then
      printf '{"workspaces":[{"id":"11111111-1111-4111-8111-111111111111","title":"%s"}]}' "$(cat "$MX_CMUX_FIXTURE/title")"
    else echo '{"workspaces":[]}'; fi ;;
  new-workspace)
    shift
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --name) shift; printf '%s' "$1" > "$MX_CMUX_FIXTURE/title" ;;
        --cwd) shift; git -C "$1" rev-parse --git-common-dir >/dev/null; printf '%s' "$1" > "$MX_CMUX_FIXTURE/cwd" ;;
      esac
      shift
    done ;;
  list-panes) echo '{"panes":[{"selected_surface_id":"22222222-2222-4222-8222-222222222222","surface_ids":["22222222-2222-4222-8222-222222222222"]}]}' ;;
  send|send-key)
    printf '%s\n' "$*" >> "$MX_CMUX_FIXTURE/sent"
    [ "${MX_CMUX_FAIL_SUBMIT:-0}" = 0 ] || exit 99 ;;
  close-workspace) rm "$MX_CMUX_FIXTURE/title" ;;
  *) echo "unexpected cmux request: $*" >&2; exit 99 ;;
esac
"#).unwrap();
        fs::set_permissions(&cmux, fs::Permissions::from_mode(0o755)).unwrap();
        let before = f.git(&["rev-parse", "HEAD"]);
        let mut command = f.spawn(&["--backend", "cmux"]);
        command
            .env("MX_CMUX_FIXTURE", &f.fake)
            .env("MX_CMUX_BIN", &cmux);
        if fail_after_metadata {
            command.env("MX_CMUX_FAIL_SUBMIT", "1");
        }
        if fail_after_endpoint {
            command.env("MX_SPAWN_FAULT", "after-endpoint");
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(i32::from(failure != "none")),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let cwd = fs::read_to_string(f.fake.join("cwd")).unwrap();
        assert_ne!(PathBuf::from(&cwd), f.project);
        assert_eq!(
            Command::new("git")
                .arg("-C")
                .arg(&cwd)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
            format!("{before}\n").as_bytes()
        );
        assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
        if fail_after_endpoint {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("injected failure after endpoint")
            );
            assert!(f.home.join("state/.spawn-task.intent").exists());
            assert!(!f.home.join("state/task.meta").exists());
            assert!(
                !f.fake.join("title").exists(),
                "failed endpoint was not closed"
            );
            assert!(
                !f.fake.join("sent").exists(),
                "harness submitted after endpoint fault"
            );
        } else if fail_after_metadata {
            let raw = fs::read_to_string(f.home.join("state/task.meta")).unwrap();
            let record =
                multplx_domain::lifecycle::subagent_model::read_meta("task", &raw).unwrap();
            assert_eq!(
                record.schedule.state,
                multplx_domain::lifecycle::subagent_model::WorkState::WaitingExternal
            );
            assert!(
                record
                    .schedule
                    .waiting_condition
                    .unwrap()
                    .contains("launch failed; reconcile retained endpoint/worktree")
            );
            assert!(f.home.join("state/.spawn-task.intent").exists());
            assert!(!f.fake.join("title").exists());
            assert!(PathBuf::from(&cwd).is_dir());
            assert!(raw.contains(&format!("worktree={cwd}\n")));
        } else {
            let raw = fs::read_to_string(f.home.join("state/task.meta")).unwrap();
            let record =
                multplx_domain::lifecycle::subagent_model::read_meta("task", &raw).unwrap();
            assert!(raw.contains(&format!("worktree={cwd}\n")));
            assert_eq!(record.attempt.unwrap().generation, 1);
            assert!(
                fs::read_to_string(f.fake.join("sent"))
                    .unwrap()
                    .contains("MX_ATTEMPT_ID=")
            );
            assert!(!f.home.join("state/.spawn-task.intent").exists());
        }
        assert!(!f.home.join("sentinel.log").exists());
    }
}
