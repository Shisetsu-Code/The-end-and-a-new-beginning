//! End-to-end CLI tests use an intentionally unknown fixture provider: no remote traffic.
use capture_runner::model::sha256;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn configuration(root: &Path, namespace: Option<&str>) -> std::path::PathBuf {
    let provider = "offline-fixture-provider";
    let evidence = root.join("evidence").join(provider);
    fs::create_dir_all(&evidence).unwrap();
    let inventory=serde_json::to_vec(&json!({"id":provider,"name":"Fixture","ready":false,"campaign_eligible":false,"games":[{"id":"fixture-game","name":"Fixture","modes":[{"key":"normal","type":"normal"}]}]})).unwrap();
    let plan = serde_json::to_vec(&json!({"casos":[]})).unwrap();
    fs::write(evidence.join("inventario.json"), &inventory).unwrap();
    fs::write(evidence.join("plan-revisado.json"), &plan).unwrap();
    fs::write(evidence.join("manifest.json"),json!({"archivos":[{"ruta":"inventario.json","sha256":sha256(&inventory)},{"ruta":"plan-revisado.json","sha256":sha256(&plan)}]}).to_string()).unwrap();
    let exits = if let Some(namespace) = namespace {
        json!([{"id":"a","namespace":namespace}])
    } else {
        json!([{"id":"a"},{"id":"b"}])
    };
    let config = root.join("runner.json");
    fs::write(&config,json!({"evidence_root":"evidence","data_root":"captures","run_id":"fixture","default_parallel_games":3,"exits":exits}).to_string()).unwrap();
    config
}
fn cli(command: &str, config: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_capture-runner"))
        .args([command, "--config"])
        .arg(config)
        .output()
        .unwrap()
}

#[test]
fn plan_is_read_only_and_reports_unsupported_without_readiness_filtering() {
    let root = tempfile::tempdir().unwrap();
    let config = configuration(root.path(), None);
    let output = cli("plan", &config);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["configured_game_slots"], 6);
    assert_eq!(plan["maximum_active_games_for_selection"], 0);
    assert_eq!(plan["providers"][0]["identified_modes"], 1);
    assert_eq!(plan["providers"][0]["unsupported_modes"], 1);
    assert!(!root.path().join("captures").exists());
}
#[test]
fn supervisor_runs_two_exits_and_status_reports_each_partial_result() {
    let root = tempfile::tempdir().unwrap();
    let config = configuration(root.path(), None);
    let output = cli("run", &config);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for exit in ["a", "b"] {
        let dir = root.path().join("captures/fixture").join(exit);
        let events = fs::read_to_string(dir.join("events.jsonl")).unwrap();
        let rows: Vec<Value> = events
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(rows.iter().any(|row| row["event"] == "unsupported"));
        assert!(
            rows.iter()
                .any(|row| row["event"] == "worker_finish" && row["counts"]["unsupported"] == 1)
        );
        assert!(!dir.join("offline-fixture-provider").exists());
    }
    let output = cli("status", &config);
    assert!(output.status.success());
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["exits"].as_array().unwrap().len(), 2);
}
#[test]
fn hidden_worker_rejects_missing_namespace_before_capture() {
    let root = tempfile::tempdir().unwrap();
    let config = configuration(root.path(), Some("crwg-missing-cli-fixture"));
    let output = Command::new(env!("CARGO_BIN_EXE_capture-runner"))
        .args(["worker", "--config"])
        .arg(config)
        .args(["--exit", "a"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!root.path().join("captures").exists());
}
