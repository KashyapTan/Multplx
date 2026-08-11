use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

#[test]
fn explicit_shadow_diagnostic_succeeds() {
    let output = Command::new(env!("CARGO_BIN_EXE_mx"))
        .arg("shadow-diagnostic")
        .output()
        .expect("run mx");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"multplx rust shadow: ready\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn argv_zero_compatibility_dispatches_the_same_command() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let alias = temp.path().join("mx-shadow-diagnostic");
    symlink(env!("CARGO_BIN_EXE_mx"), &alias).expect("create multicall symlink");

    let output = Command::new(alias).output().expect("run multicall alias");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"multplx rust shadow: ready\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn help_succeeds_and_advertises_ported_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_mx"))
        .arg("--help")
        .output()
        .expect("run help");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");

    assert!(output.status.success());
    assert!(stdout.contains("Multplx broker runtime"));
    assert!(stdout.contains("backlog"));
    assert!(stdout.contains("config-push"));
    assert!(!stdout.contains("shadow-diagnostic"));
}

#[test]
fn missing_and_unknown_commands_are_usage_errors() {
    for args in [Vec::<&str>::new(), vec!["not-ported"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_mx"))
            .args(args)
            .output()
            .expect("run invalid invocation");

        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn unknown_multicall_alias_is_a_usage_error() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let alias = temp.path().join("mx-not-ported");
    symlink(env!("CARGO_BIN_EXE_mx"), &alias).expect("create multicall symlink");

    let output = Command::new(alias).output().expect("run unknown alias");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    fs::remove_file(temp.path().join("mx-not-ported")).expect("remove alias");
}
