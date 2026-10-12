// Export the production bundle and isolated canonical report-task fixtures.
// Usage: codex-hook-arguments ABS_RUNTIME ABS_MX ABS_OPERATIONAL_HOME
use multplx_domain::lifecycle::subagent_model::{
    ArtifactKind, AssignmentRole, TaskRecord, write_meta,
};
use std::path::Path;
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let root = Path::new(&args[0]);
    let bundle = multplx_core::codex_hooks::arguments(root, Path::new(&args[1])).unwrap();
    let home = Path::new(&args[2]);
    let parent = format!("root-home:{}", home.display());
    let mut record = TaskRecord::new(
        "fixture-child".into(),
        AssignmentRole::Researcher,
        ArtifactKind::Report,
        true,
        parent.clone(),
        parent,
        home.display().to_string(),
    );
    record.owner_state = Some(home.join("state").display().to_string());
    record.parent_state = record.owner_state.clone();
    println!(
        "{}",
        serde_json::json!({"arguments":bundle.iter().map(|arg|arg.to_str().unwrap()).collect::<Vec<_>>(),"metadata":write_meta("", &record).unwrap(),"attempt":record.attempt})
    );
}
