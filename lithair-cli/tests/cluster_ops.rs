#![cfg(feature = "cluster-ops")]
#[path = "../../lithair-core/tests/support/operator_files.rs"]
mod fixture;
use std::{
    path::Path,
    process::{Command, Output},
};

fn run(action: &str, config: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lithair"))
        .args(["cluster", action, "--config"])
        .arg(config)
        .current_dir(config.parent().unwrap().parent().unwrap())
        .output()
        .unwrap()
}
#[test]
fn offline_commands_have_structured_output_and_scriptable_errors() {
    let files = fixture::OperatorFiles::new();
    for action in ["check", "provision", "inspect"] {
        let output = run(action, &files.configs[&1]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["action"], action);
        assert_eq!(value["node_id"], 1);
        assert_eq!(value["peer_ids"], serde_json::json!([1, 2, 3]));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE KEY"));
    }
    assert!(files.data(1).join("durable.meta").exists());
    let output = run("provision", &files.configs[&1]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let output = run("inspect", &files.configs[&2]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!files.data(2).exists());
}
#[test]
fn parse_errors_do_not_echo_sensitive_configuration_contents() {
    let files = fixture::OperatorFiles::new();
    let path = &files.configs[&1];
    std::fs::write(path, "unknown_field = 'SECRET_TEST_VALUE'\n").unwrap();
    let output = run("check", path);
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("SECRET_TEST_VALUE"));
    assert!(!files.data(1).exists());
}
