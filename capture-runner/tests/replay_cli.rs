//! The public replay commands operate only on local files and never open a provider session.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_capture-runner"))
        .args(args)
        .output()
        .unwrap()
}

fn captured_attempt(root: &Path, name: &str, complete: bool) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("attempt.json"),
        json!({"provider":"fixture","game":"slot","mode":"pick","sample":0}).to_string(),
    )
    .unwrap();
    let request = json!({"label":"choice","url":"wss://example.invalid/game","kind":"text","raw":"{\"action\":\"pick\",\"index\":2}","round_action":true});
    let mut lines = vec![
        json!({"seq":1,"at_ms":1,"event":"attempt_start","data":{"game":"slot"}}),
        json!({"seq":2,"at_ms":2,"event":"ws_send","data":request}),
        json!({"seq":3,"at_ms":3,"event":"ws_receive","data":{"label":"choice","url":"wss://example.invalid/game","kind":"binary_base64","raw":"AP8QgA=="}}),
    ];
    if complete {
        lines
            .push(json!({"seq":4,"at_ms":4,"event":"attempt_finish","data":{"status":"complete"}}));
        fs::write(dir.join("result.json"), "{\"status\":\"complete\"}\n").unwrap();
    }
    let mut wire = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    if !complete {
        wire.push_str("{\"seq\":4,\"event\":");
    }
    fs::write(dir.join("wire.jsonl"), wire).unwrap();
}

#[test]
fn export_preserves_partial_bytes_and_replay_requires_the_recorded_choice() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("captures");
    let bank = root.path().join("bank");
    captured_attempt(&source, "closed", true);
    captured_attempt(&source, "partial", false);
    let output = run(&[
        "export",
        "--input",
        source.to_str().unwrap(),
        "--output",
        bank.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let index: Value = serde_json::from_slice(&fs::read(bank.join("index.json")).unwrap()).unwrap();
    let attempts = index["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    let closed = attempts.iter().find(|a| a["state"] == "complete").unwrap();
    let partial = attempts.iter().find(|a| a["state"] == "partial").unwrap();
    let partial_wire = bank
        .join("attempts")
        .join(partial["id"].as_str().unwrap())
        .join("wire.jsonl");
    assert_eq!(
        fs::read(partial_wire).unwrap(),
        fs::read(source.join("partial/wire.jsonl")).unwrap()
    );
    assert!(!partial["issues"].as_array().unwrap().is_empty());
    let template = run(&[
        "replay",
        "--bank",
        bank.to_str().unwrap(),
        "--attempt",
        closed["id"].as_str().unwrap(),
        "--emit-requests",
    ]);
    assert!(
        template.status.success(),
        "{}",
        String::from_utf8_lossy(&template.stderr)
    );
    let emitted: Value = serde_json::from_slice(&template.stdout).unwrap();
    assert_eq!(emitted["event"], "ws_send");
    assert_eq!(emitted["data"]["raw"], "{\"action\":\"pick\",\"index\":2}");
    let requests = root.path().join("requests.jsonl");
    let request = json!({"event":"ws_send","data":{"url":"wss://example.invalid/game","kind":"text","raw":"{\"action\":\"pick\",\"index\":2}"}});
    fs::write(&requests, request.to_string() + "\n").unwrap();
    let args = [
        "replay",
        "--bank",
        bank.to_str().unwrap(),
        "--attempt",
        closed["id"].as_str().unwrap(),
        "--requests",
        requests.to_str().unwrap(),
    ];
    let output = run(&args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let step = rows.iter().find(|r| r["event"] == "replay_step").unwrap();
    assert_eq!(step["events"][0]["data"]["raw"], "AP8QgA==");
    assert_eq!(
        STANDARD
            .decode(step["events"][0]["data"]["raw"].as_str().unwrap())
            .unwrap(),
        vec![0, 255, 16, 128]
    );
    fs::write(
        &requests,
        request.to_string().replace("index\\\":2", "index\\\":3") + "\n",
    )
    .unwrap();
    let rejected = run(&args);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&rejected.stdout).contains("AP8QgA=="));
    let again = run(&[
        "export",
        "--input",
        source.to_str().unwrap(),
        "--output",
        bank.to_str().unwrap(),
    ]);
    assert_eq!(again.status.code(), Some(1));
}

#[test]
fn modified_bank_is_rejected_before_releasing_a_response() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("captures");
    let bank = root.path().join("bank");
    captured_attempt(&source, "closed", true);
    let output = run(&[
        "export",
        "--input",
        source.to_str().unwrap(),
        "--output",
        bank.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let index: Value = serde_json::from_slice(&fs::read(bank.join("index.json")).unwrap()).unwrap();
    let id = index["attempts"][0]["id"].as_str().unwrap();
    fs::write(bank.join("attempts").join(id).join("wire.jsonl"), "changed").unwrap();
    let output = run(&["replay", "--bank", bank.to_str().unwrap(), "--attempt", id]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("SHA-256"));
}

#[test]
fn review_imported_future_outcomes_are_not_released_before_matching() {
    use capture_runner::{
        replay::{self, ReplaySession},
        ruby_import,
    };
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rubyplay/native-complete.json")).unwrap();
    let capture = &fixtures
        .iter()
        .find(|f| f["capture"]["game"] == "rp_120")
        .unwrap()["capture"];
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("rp_120.json");
    let bank_path = root.path().join("bank");
    fs::write(&input, serde_json::to_vec(capture).unwrap()).unwrap();
    let bank = ruby_import::import(&input, &bank_path).unwrap();
    let attempt = bank
        .attempts
        .iter()
        .find(|a| a.metadata["sample"] == 0)
        .unwrap();
    assert!(!attempt.metadata["source_round"]["balance_after"].is_null());
    assert!(
        !attempt.metadata["source_context"]["errors"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let session = ReplaySession::open(&bank_path, attempt).unwrap();
    assert!(
        !session
            .initial_events()
            .iter()
            .any(|e| e["event"] == "attempt_start")
    );
    let mut wrong = session.expected_request().unwrap().clone();
    wrong["data"]["action"] = json!("incorrect_action");
    let requests = root.path().join("wrong.jsonl");
    fs::write(&requests, wrong.to_string() + "\n").unwrap();
    let output = run(&[
        "replay",
        "--bank",
        bank_path.to_str().unwrap(),
        "--attempt",
        &attempt.id,
        "--requests",
        requests.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let start: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(start["event"], "replay_start");
    assert_eq!(start["metadata"]["game"], "rp_120");
    for key in [
        "source_round",
        "source_context",
        "balance_after",
        "closed",
        "errors",
    ] {
        assert!(!String::from_utf8_lossy(&output.stdout).contains(&format!("\"{key}\"")));
    }
    assert!(start["events"].as_array().unwrap().is_empty());
    let inspected = replay::inspect(&bank_path, attempt).unwrap();
    assert_eq!(
        inspected["metadata"]["source_round"],
        attempt.metadata["source_round"]
    );
    assert_eq!(
        inspected["metadata"]["source_context"]["errors"],
        capture["errors"]
    );
    let mut matching = ReplaySession::open(&bank_path, attempt).unwrap();
    let req = matching.expected_request().unwrap().clone();
    let step = matching.submit(&req).unwrap();
    assert_eq!(
        step["events"][0]["data"],
        capture["rounds"][0]["steps"][0]["response"]
    );
}
