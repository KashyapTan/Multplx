//! Repository workspace placement, independent of task ownership and standing homes.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use multplx_core::filesystem::{atomic_replace, read_bounded_regular};
use multplx_core::locks::DirectoryLock;
use multplx_core::process::SystemProcessProbe;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::command::{CommandRequest, CommandRunner, SystemCommandRunner};
use crate::facade::{BackendError, BackendName, ContainerId, RuntimeBackend, TaskSpec};
use crate::herdr::{HerdrBackend, HerdrTaskEndpoint};
use crate::herdr_presentation::projection_id;

/// Read the explicit legacy-layout escape hatch; absent means repository grouping.
pub fn repository_layout(config: &Path) -> Result<bool, BackendError> {
    let path = config.join("herdr-workspace-layout");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(metadata(error)),
        Ok(_) => {
            let bytes = read_bounded_regular(path, 128).map_err(metadata)?;
            match std::str::from_utf8(&bytes).map_err(metadata)?.trim() {
                "repository" => Ok(true),
                "home" => Ok(false),
                _ => Err(metadata(
                    "config/herdr-workspace-layout must be repository or home",
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandError, CommandOutput};
    use serde_json::json;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, ExitStatus};
    use std::sync::{Arc, Mutex};

    #[derive(Debug)]
    struct World {
        socket: PathBuf,
        spaces: Vec<Value>,
        tabs: Vec<Value>,
        calls: Vec<Vec<String>>,
        lost_create: bool,
    }
    #[derive(Debug, Clone)]
    struct Fake(Arc<Mutex<World>>);
    impl CommandRunner for Fake {
        fn run(&mut self, request: &CommandRequest) -> Result<CommandOutput, CommandError> {
            let args = request
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let mut world = self.0.lock().unwrap();
            world.calls.push(args.clone());
            let arg = |name: &str| {
                args.iter()
                    .position(|value| value == name)
                    .map(|index| args[index + 1].clone())
            };
            let value = match (args[0].as_str(), args.get(1).map(String::as_str)) {
                ("--version", _) => {
                    return Ok(CommandOutput {
                        status: ExitStatus::from_raw(0),
                        stdout: b"herdr 0.7.4\n".to_vec(),
                        stderr: vec![],
                    });
                }
                ("status", _) => {
                    json!({"client":{"version":"0.7.4","protocol":14},"server":{"running":true}})
                }
                ("session", _) => {
                    json!({"sessions":[{"name":"named","socket_path":world.socket,"running":true}]})
                }
                ("workspace", Some("list")) => json!({"result":{"workspaces":world.spaces}}),
                ("workspace", Some("create")) => {
                    assert!(args.contains(&"--no-focus".to_owned()));
                    let id = format!("w{}", world.spaces.len());
                    let tab = format!("{id}:seed");
                    world
                        .spaces
                        .push(json!({"workspace_id":id,"label":arg("--label"),"focused":false}));
                    world
                        .tabs
                        .push(json!({"workspace_id":id,"tab_id":tab,"label":"1","focused":false}));
                    if world.lost_create {
                        world.lost_create = false;
                        json!({"result":{}})
                    } else {
                        json!({"result":{"workspace":{"workspace_id":id},"tab":{"tab_id":tab}}})
                    }
                }
                ("workspace", Some("rename")) => {
                    let space = world
                        .spaces
                        .iter_mut()
                        .find(|value| value["workspace_id"] == args[2])
                        .unwrap();
                    space["label"] = json!(args[3]);
                    json!({})
                }
                ("tab", Some("list")) => {
                    json!({"result":{"tabs":world.tabs.iter().filter(|value| value["workspace_id"] == arg("--workspace").unwrap()).collect::<Vec<_>>()}})
                }
                ("tab", Some("create")) => {
                    assert!(args.contains(&"--no-focus".to_owned()));
                    let id = arg("--workspace").unwrap();
                    let tab = format!("{id}:t{}", world.calls.len());
                    world.tabs.push(json!({"workspace_id":id,"tab_id":tab,"label":arg("--label"),"focused":false}));
                    json!({"result":{"tab":{"tab_id":tab},"root_pane":{"pane_id":format!("{tab}:p")}}})
                }
                ("pane", Some("list")) => {
                    json!({"result":{"panes":world.tabs.iter().filter(|value| value["workspace_id"] == arg("--workspace").unwrap()).map(|tab| json!({"workspace_id":tab["workspace_id"],"tab_id":tab["tab_id"],"pane_id":format!("{}:p",tab["tab_id"].as_str().unwrap())})).collect::<Vec<_>>()}})
                }
                ("pane", Some("close")) => {
                    world
                        .tabs
                        .retain(|tab| format!("{}:p", tab["tab_id"].as_str().unwrap()) != args[2]);
                    json!({})
                }
                ("pane", Some("get")) => {
                    world.tabs.iter().find(|tab| format!("{}:p", tab["tab_id"].as_str().unwrap()) == args[2])
                        .map_or_else(|| json!({"error":{"code":"pane_not_found"}}), |tab| json!({"result":{"pane":{"pane_id":args[2],"tab_id":tab["tab_id"],"workspace_id":tab["workspace_id"]}}}))
                }
                ("agent", _) => json!({"error":{"code":"agent_not_found"}}),
                other => panic!("unexpected fake call {other:?}: {args:?}"),
            };
            Ok(CommandOutput {
                status: ExitStatus::from_raw(0),
                stdout: serde_json::to_vec(&value).unwrap(),
                stderr: vec![],
            })
        }
    }
    struct Fixture {
        temp: tempfile::TempDir,
        root: PathBuf,
        repo: PathBuf,
        common: PathBuf,
        fake: Fake,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(temp.path()).unwrap().join("home");
            fs::create_dir_all(root.join("state")).unwrap();
            let repo = temp.path().join("project");
            fs::create_dir(&repo).unwrap();
            git(&repo, &["init", "-q"]);
            git(
                &repo,
                &[
                    "-c",
                    "user.name=Tests",
                    "-c",
                    "user.email=tests@example.invalid",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "initial",
                ],
            );
            let common = repository(&repo).unwrap().unwrap();
            let fake = Fake(Arc::new(Mutex::new(World {
                socket: temp.path().join("herdr.sock"),
                spaces: vec![
                    json!({"workspace_id":"human","label":"project","focused":true,"active_tab_id":"human:t"}),
                ],
                tabs: vec![
                    json!({"workspace_id":"human","tab_id":"human:t","label":"human","focused":true}),
                ],
                calls: vec![],
                lost_create: false,
            })));
            Self {
                temp,
                root,
                repo,
                common,
                fake,
            }
        }
        fn create(
            &self,
            home: &Path,
            label: &str,
        ) -> Result<(String, HerdrTaskEndpoint, DirectoryLock), BackendError> {
            HerdrBackend::new(self.fake.clone(), "herdr", "named", home.to_path_buf())
                .project_task_create(
                    &self.root,
                    &self.common,
                    &self.repo,
                    home,
                    &TaskSpec {
                        label: label.into(),
                        working_directory: self.repo.clone(),
                    },
                )
        }
        fn binding(&self) -> PathBuf {
            self.root
                .join("state/herdr-project-workspaces")
                .join(format!("{}.json", key(&self.root, &self.common, "named")))
        }
    }
    fn git(cwd: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    #[test]
    fn worktrees_share_identity_and_same_basename_repositories_do_not() {
        let f = Fixture::new();
        let linked = f.temp.path().join("detached");
        git(
            &f.repo,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );
        assert_eq!(repository(&linked).unwrap(), Some(f.common.clone()));
        let alias = f.temp.path().join("alias");
        symlink(&linked, &alias).unwrap();
        assert_eq!(repository(&alias).unwrap(), Some(f.common.clone()));
        let other = f.temp.path().join("other/project");
        fs::create_dir_all(&other).unwrap();
        git(&other, &["init", "-q"]);
        assert_ne!(repository(&other).unwrap(), Some(f.common));
        assert_eq!(repository(f.temp.path()).unwrap(), None);
    }
    #[test]
    fn layout_values_are_explicit_bounded_and_never_follow_symlinks() {
        let f = Fixture::new();
        let dir = f.root.join("config");
        fs::create_dir(&dir).unwrap();
        assert!(repository_layout(&dir).unwrap());
        for (value, grouped) in [("home\n", false), ("repository\n", true)] {
            fs::write(dir.join("herdr-workspace-layout"), value).unwrap();
            assert_eq!(repository_layout(&dir).unwrap(), grouped);
        }
        for value in ["", "per-agent", "HOME", "repository\nhome"] {
            fs::write(dir.join("herdr-workspace-layout"), value).unwrap();
            assert!(repository_layout(&dir).is_err());
        }
        fs::remove_file(dir.join("herdr-workspace-layout")).unwrap();
        symlink("/dev/null", dir.join("herdr-workspace-layout")).unwrap();
        assert!(repository_layout(&dir).is_err());
    }
    #[test]
    fn nested_homes_share_workspace_without_label_adoption_or_duplicate_cleanup() {
        let f = Fixture::new();
        let nested = f.temp.path().join("standing");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join(".mx-agent-home"), "worker\n").unwrap();
        let (first, ep, lock) = f.create(&f.root, "mx-same").unwrap();
        drop(lock);
        let (second, child, lock) = f.create(&nested, "mx-same").unwrap();
        drop(lock);
        assert_eq!(first, second);
        assert_ne!(ep.tab_id, child.tab_id);
        assert!(f.create(&f.root, "mx-same").is_err());
        let world = f.fake.0.lock().unwrap();
        assert_eq!(world.spaces.len(), 2);
        assert_eq!(world.spaces[0]["label"], "project");
        assert_eq!(
            world
                .tabs
                .iter()
                .filter(|tab| tab["workspace_id"] == first)
                .count(),
            2
        );
        assert!(
            !world
                .calls
                .iter()
                .any(|args| args[0] == "tab" && args[1] == "close")
        );
        assert_eq!(
            world
                .calls
                .iter()
                .filter(|args| args[0] == "workspace" && args[1] == "rename")
                .count(),
            1
        );
    }
    #[test]
    fn uncertain_create_reconciles_own_nonce_without_seed_pruning() {
        let f = Fixture::new();
        f.fake.0.lock().unwrap().lost_create = true;
        assert!(f.create(&f.root, "mx-task").is_err());
        let (_, _, lock) = f.create(&f.root, "mx-task").unwrap();
        drop(lock);
        let world = f.fake.0.lock().unwrap();
        assert_eq!(world.spaces.len(), 2);
        assert_eq!(
            world.tabs.iter().filter(|tab| tab["label"] == "1").count(),
            1
        );
        assert!(
            !world
                .calls
                .iter()
                .any(|args| args[0] == "pane" && args[1] == "close")
        );
    }
    #[test]
    fn missing_workspace_recreates_but_corrupt_inventory_never_means_absent() {
        let f = Fixture::new();
        let (_, _, lock) = f.create(&f.root, "mx-one").unwrap();
        drop(lock);
        f.fake.0.lock().unwrap().spaces.pop();
        let (_, _, lock) = f.create(&f.root, "mx-two").unwrap();
        drop(lock);
        assert_eq!(f.fake.0.lock().unwrap().spaces.len(), 2);
        for row in [
            json!({"label":"foreign"}),
            json!({"workspace_id":""}),
            json!({"workspace_id":"human"}),
        ] {
            f.fake.0.lock().unwrap().spaces.push(row);
            assert!(f.create(&f.root, "mx-three").is_err());
            f.fake.0.lock().unwrap().spaces.pop();
        }
    }
    #[test]
    fn ambiguous_pending_create_and_foreign_binding_fail_closed() {
        let f = Fixture::new();
        f.fake.0.lock().unwrap().lost_create = true;
        assert!(f.create(&f.root, "mx-one").is_err());
        let binding: Binding = serde_json::from_slice(&fs::read(f.binding()).unwrap()).unwrap();
        f.fake
            .0
            .lock()
            .unwrap()
            .spaces
            .push(json!({"workspace_id":"evil","label":binding.create_label}));
        assert!(f.create(&f.root, "mx-two").is_err());
        f.fake.0.lock().unwrap().spaces.pop();
        for bad in [
            Binding {
                version: 2,
                ..binding.clone()
            },
            Binding {
                session: "foreign".into(),
                ..binding.clone()
            },
            Binding {
                create_label: "mx-project:......................".into(),
                ..binding.clone()
            },
            Binding {
                workspace_id: Some(String::new()),
                ..binding.clone()
            },
        ] {
            write(&f.binding(), &bad).unwrap();
            assert!(f.create(&f.root, "mx-two").is_err());
        }
        write(&f.binding(), &binding).unwrap();
        fs::set_permissions(f.binding(), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(f.create(&f.root, "mx-two").is_err());
        fs::remove_file(f.binding()).unwrap();
        symlink("/dev/null", f.binding()).unwrap();
        assert!(f.create(&f.root, "mx-two").is_err());
    }
    #[test]
    fn concurrent_launches_converge_under_shared_session_lock() {
        let f = Arc::new(Fixture::new());
        let mut threads = vec![];
        for n in 0..4 {
            let f = f.clone();
            threads.push(std::thread::spawn(move || {
                let (workspace, _, lock) = f.create(&f.root, &format!("mx-{n}")).unwrap();
                drop(lock);
                workspace
            }));
        }
        let ids = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), 1);
        assert_eq!(f.fake.0.lock().unwrap().spaces.len(), 2);
    }

    #[test]
    fn shared_recovery_replaces_only_exact_single_agent_free_owned_pane() {
        let f = Fixture::new();
        let (workspace, endpoint, lock) = f.create(&f.root, "mx-recover").unwrap();
        drop(lock);
        let spec = TaskSpec {
            label: endpoint.target.expected_label().unwrap().to_owned(),
            working_directory: f.repo.clone(),
        };
        let container =
            ContainerId::for_backend(BackendName::Herdr, format!("named:{workspace}")).unwrap();
        let recover = || {
            HerdrBackend::new(f.fake.clone(), "herdr", "named", f.root.clone())
                .create_task_recover_owned(&container, &spec, &endpoint.tab_id, &endpoint.pane_id)
        };
        let original = f
            .fake
            .0
            .lock()
            .unwrap()
            .tabs
            .iter()
            .find(|tab| tab["tab_id"] == endpoint.tab_id)
            .unwrap()
            .clone();
        let mut foreign = original.clone();
        foreign["tab_id"] = json!("foreign:t");
        f.fake.0.lock().unwrap().tabs.push(foreign);
        assert!(recover().is_err());
        f.fake.0.lock().unwrap().tabs.pop();
        f.fake.0.lock().unwrap().tabs.push(original.clone());
        assert!(recover().is_err());
        f.fake.0.lock().unwrap().tabs.pop();
        f.fake
            .0
            .lock()
            .unwrap()
            .tabs
            .iter_mut()
            .find(|tab| tab["tab_id"] == endpoint.tab_id)
            .unwrap()["label"] = json!("renamed");
        assert!(recover().is_err());
        f.fake
            .0
            .lock()
            .unwrap()
            .tabs
            .iter_mut()
            .find(|tab| tab["tab_id"] == endpoint.tab_id)
            .unwrap()["label"] = json!(spec.label);
        let replacement = recover().unwrap();
        assert_ne!(replacement.pane_id, endpoint.pane_id);
        let world = f.fake.0.lock().unwrap();
        assert!(
            !world
                .tabs
                .iter()
                .any(|tab| tab["tab_id"] == endpoint.tab_id)
        );
        assert!(world.tabs.iter().any(|tab| tab["tab_id"] == "human:t"));
        assert!(
            !world
                .calls
                .iter()
                .any(|args| args[0] == "tab" && args[1] == "close")
        );
    }

    #[test]
    fn shared_recovery_refuses_active_target_before_creating_replacement() {
        let f = Fixture::new();
        let (workspace, endpoint, lock) = f.create(&f.root, "mx-recover").unwrap();
        drop(lock);
        {
            let mut world = f.fake.0.lock().unwrap();
            world.spaces[0]["focused"] = json!(false);
            world.spaces[1]["focused"] = json!(true);
            world.spaces[1]["active_tab_id"] = json!(endpoint.tab_id);
            for tab in &mut world.tabs {
                tab["focused"] = json!(tab["tab_id"] == endpoint.tab_id);
            }
        }
        let before = f.fake.0.lock().unwrap().tabs.len();
        let spec = TaskSpec {
            label: endpoint.target.expected_label().unwrap().into(),
            working_directory: f.repo.clone(),
        };
        let container =
            ContainerId::for_backend(BackendName::Herdr, format!("named:{workspace}")).unwrap();
        assert!(
            HerdrBackend::new(f.fake.clone(), "herdr", "named", f.root.clone())
                .create_task_recover_owned(&container, &spec, &endpoint.tab_id, &endpoint.pane_id)
                .is_err()
        );
        assert_eq!(f.fake.0.lock().unwrap().tabs.len(), before);
    }

    #[test]
    fn root_scopes_and_distinct_repositories_get_distinct_verified_workspaces() {
        let f = Fixture::new();
        let (first, _, lock) = f.create(&f.root, "mx-one").unwrap();
        drop(lock);
        let other_root = f.temp.path().join("other-home");
        fs::create_dir_all(other_root.join("state")).unwrap();
        let (second, _, lock) =
            HerdrBackend::new(f.fake.clone(), "herdr", "named", other_root.clone())
                .project_task_create(
                    &other_root,
                    &f.common,
                    &f.repo,
                    &other_root,
                    &TaskSpec {
                        label: "mx-two".into(),
                        working_directory: f.repo.clone(),
                    },
                )
                .unwrap();
        drop(lock);
        assert_ne!(first, second);
        let other = f.temp.path().join("another/project");
        fs::create_dir_all(&other).unwrap();
        git(&other, &["init", "-q"]);
        let common = repository(&other).unwrap().unwrap();
        let (third, _, lock) = HerdrBackend::new(f.fake.clone(), "herdr", "named", f.root.clone())
            .project_task_create(
                &f.root,
                &common,
                &other,
                &f.root,
                &TaskSpec {
                    label: "mx-three".into(),
                    working_directory: other.clone(),
                },
            )
            .unwrap();
        drop(lock);
        assert_ne!(first, third);
        assert!(
            HerdrBackend::new(f.fake.clone(), "herdr", "named", f.root.clone())
                .project_task_create(
                    &f.root,
                    &f.common,
                    &other,
                    &f.root,
                    &TaskSpec {
                        label: "mx-four".into(),
                        working_directory: other.clone()
                    }
                )
                .is_err()
        );
    }
}

/// Canonical repository identity shared by linked and detached worktrees.
pub fn repository(cwd: &Path) -> Result<Option<PathBuf>, BackendError> {
    let output = SystemCommandRunner
        .run(&CommandRequest::new(
            "git",
            [
                OsString::from("-C"),
                cwd.as_os_str().to_owned(),
                OsString::from("rev-parse"),
                OsString::from("--path-format=absolute"),
                OsString::from("--git-common-dir"),
            ],
        ))
        .map_err(|error| BackendError::Metadata(error.to_string()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|error| BackendError::Metadata(error.to_string()))?;
    let path = PathBuf::from(text.trim_end_matches('\n'));
    if !path.is_absolute() || text.trim_end_matches('\n').contains(['\n', '\r']) {
        return Err(BackendError::Metadata(
            "invalid Git common directory".to_owned(),
        ));
    }
    fs::canonicalize(path).map(Some).map_err(metadata)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    version: u8,
    root_home: PathBuf,
    repository: PathBuf,
    session: String,
    create_label: String,
    workspace_id: Option<String>,
}

fn metadata(error: impl std::fmt::Display) -> BackendError {
    BackendError::Metadata(error.to_string())
}

fn key(root: &Path, repository: &Path, session: &str) -> String {
    let mut hash = Sha256::new();
    for bytes in [
        root.as_os_str().as_encoded_bytes(),
        repository.as_os_str().as_encoded_bytes(),
        session.as_bytes(),
    ] {
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    format!("{:x}", hash.finalize())
}

fn private_directory(path: &Path) -> Result<(), BackendError> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(metadata(error)),
    }
    let info = fs::symlink_metadata(path).map_err(metadata)?;
    if !info.is_dir()
        || info.file_type().is_symlink()
        || info.uid() != rustix::process::getuid().as_raw()
        || info.mode() & 0o777 != 0o700
    {
        return Err(metadata("repository workspace namespace is not private"));
    }
    Ok(())
}

fn write(path: &Path, binding: &Binding) -> Result<(), BackendError> {
    atomic_replace(path, &serde_json::to_vec(binding).map_err(metadata)?, 0o600).map_err(metadata)
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.pointer(name).and_then(Value::as_str)
}

impl<R: CommandRunner> HerdrBackend<R> {
    /// Create a fresh repository task under a root supplied by canonical admission.
    /// The caller retains the session lock until exact endpoint metadata is published.
    pub fn project_task_create(
        &mut self,
        root_home: &Path,
        common_dir: &Path,
        project_checkout: &Path,
        owner_home: &Path,
        task: &TaskSpec,
    ) -> Result<(String, HerdrTaskEndpoint, DirectoryLock), BackendError> {
        self.version_check()?;
        let session = self.session().to_owned();
        self.server_ensure(&session)?;
        let lock = DirectoryLock::acquire_wait(
            self.presentation_session_lock_path(&session)?,
            &SystemProcessProbe::default(),
            Duration::from_secs(15),
        )
        .map_err(metadata)?;
        let root = fs::canonicalize(root_home).map_err(metadata)?;
        let common = fs::canonicalize(common_dir).map_err(metadata)?;
        if repository(project_checkout)?.as_ref() != Some(&common) {
            return Err(metadata(
                "task checkout repository changed before workspace creation",
            ));
        }
        let directory = root.join("state/herdr-project-workspaces");
        private_directory(&directory)?;
        let path = directory.join(format!("{}.json", key(&root, &common, &session)));
        let mut binding = match fs::symlink_metadata(&path) {
            Ok(info) => {
                if info.uid() != rustix::process::getuid().as_raw() || info.mode() & 0o777 != 0o600
                {
                    return Err(metadata("repository workspace binding is not private"));
                }
                serde_json::from_slice::<Binding>(
                    &read_bounded_regular(&path, 64 * 1024).map_err(metadata)?,
                )
                .map_err(metadata)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Binding {
                version: 1,
                root_home: root.clone(),
                repository: common.clone(),
                session: session.clone(),
                create_label: format!("mx-project:{}", projection_id()?),
                workspace_id: None,
            },
            Err(error) => return Err(metadata(error)),
        };
        if binding.version != 1
            || binding.root_home != root
            || binding.repository != common
            || binding.session != session
            || !binding.create_label.starts_with("mx-project:")
            || binding.create_label.len() != 33
            || !binding.create_label[11..]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || binding
                .workspace_id
                .as_ref()
                .is_some_and(|id| id.is_empty())
        {
            return Err(metadata("repository workspace binding identity mismatch"));
        }
        let snapshot = self.json_scoped(&session, ["workspace", "list"])?;
        let workspaces = snapshot
            .pointer("/result/workspaces")
            .and_then(Value::as_array)
            .ok_or_else(|| metadata("unreadable repository workspace inventory"))?;
        let mut identities = std::collections::BTreeSet::new();
        for workspace in workspaces {
            let id = field(workspace, "/workspace_id")
                .filter(|id| !id.is_empty())
                .ok_or_else(|| metadata("workspace inventory contains an invalid identity"))?;
            if !identities.insert(id) {
                return Err(metadata(
                    "workspace inventory contains duplicate exact identities",
                ));
            }
        }
        let focus = if workspaces.is_empty() {
            None
        } else {
            Some(self.focus_snapshot(&session)?)
        };
        let mut seed = None;
        let workspace = if let Some(id) = &binding.workspace_id {
            let matches = workspaces
                .iter()
                .filter(|value| field(value, "/workspace_id") == Some(id))
                .count();
            if matches > 1 {
                return Err(metadata("ambiguous exact repository workspace"));
            }
            if matches == 1 { Some(id.clone()) } else { None }
        } else {
            // Only this durable pre-create random intent can reconcile an uncertain create.
            // Ordinary repository titles are never searched or adopted.
            let matches = workspaces
                .iter()
                .filter(|value| field(value, "/label") == Some(&binding.create_label))
                .collect::<Vec<_>>();
            if matches.len() > 1 {
                return Err(metadata("ambiguous pending repository workspace creation"));
            }
            matches
                .first()
                .map(|value| {
                    field(value, "/workspace_id")
                        .map(str::to_owned)
                        .ok_or_else(|| metadata("pending workspace has no exact id"))
                })
                .transpose()?
        };
        let workspace = if let Some(workspace) = workspace {
            workspace
        } else {
            // A positively missing old workspace may be replaced. A new nonce makes a
            // retired intent incapable of adopting an unrelated workspace later.
            if binding.workspace_id.is_some() {
                binding.create_label = format!("mx-project:{}", projection_id()?);
                binding.workspace_id = None;
            }
            write(&path, &binding)?;
            let created = self.json_scoped(
                &session,
                [
                    OsString::from("workspace"),
                    OsString::from("create"),
                    OsString::from("--cwd"),
                    task.working_directory.as_os_str().to_owned(),
                    OsString::from("--label"),
                    OsString::from(&binding.create_label),
                    OsString::from("--no-focus"),
                ],
            )?;
            let id = field(&created, "/result/workspace/workspace_id")
                .filter(|id| !id.is_empty())
                .ok_or_else(|| metadata("workspace create omitted exact identity"))?
                .to_owned();
            seed = field(&created, "/result/tab/tab_id").map(str::to_owned);
            id
        };
        let first_binding = binding.workspace_id.is_none();
        binding.workspace_id = Some(workspace.clone());
        write(&path, &binding)?;
        // Bind the response id durably before changing its temporary create token.
        // Renaming is presentation-only and failure never creates a second container.
        let repo_name = common
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("repository");
        let title = if workspaces.iter().any(|value| {
            field(value, "/label") == Some(repo_name)
                && field(value, "/workspace_id") != Some(workspace.as_str())
        }) {
            format!("{repo_name} ({})", &key(&root, &common, &session)[..8])
        } else {
            repo_name.to_owned()
        };
        if first_binding {
            let _ = self.run_scoped(&session, ["workspace", "rename", &workspace, &title]);
        }
        let owner = fs::canonicalize(owner_home).map_err(metadata)?;
        let owner_key = key(&root, &owner, &session);
        let qualified = TaskSpec {
            label: format!("{} [{}]", task.label, &owner_key[..12]),
            working_directory: task.working_directory.clone(),
        };
        let container =
            ContainerId::for_backend(BackendName::Herdr, format!("{session}:{workspace}"))?;
        let endpoint = self.create_task_fresh(&container, &qualified, seed.as_deref())?;
        if let Some(focus) = focus {
            self.focus_restore(&session, &focus)?;
        }
        Ok((workspace, endpoint, lock))
    }
}
