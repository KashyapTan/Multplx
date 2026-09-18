//! Phase 11 shell contracts routed through the instrumented native binary.

use std::path::Path;
use std::process::Command;

fn run_contract(script: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("bash")
        .arg(root.join(script))
        .current_dir(&root)
        .env("MX_RUST_BIN", env!("CARGO_BIN_EXE_mx"))
        .env("MX_RUST_SOURCE_ROOT", &root)
        .output()
        .unwrap_or_else(|error| panic!("run {script}: {error}"));
    assert!(
        output.status.success(),
        "{script} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn phase11_shell_contracts_use_the_instrumented_binary() {
    if std::env::var_os("LLVM_PROFILE_FILE").is_none() {
        return;
    }
    for script in [
        "tests/mx-workspace-discovery.test.sh",
        "tests/mx-release-package.test.sh",
        "tests/mx-launcher.test.sh",
        "tests/mx-launcher-shell.test.sh",
    ] {
        run_contract(script);
    }
}
