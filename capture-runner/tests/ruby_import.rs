//! Historical parsed native messages remain separate from original HTTP evidence.
use capture_runner::{capture_bank, replay::ReplaySession, ruby_import};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

fn fixture(game: &str) -> Value {
    let entries: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/rubyplay/native-complete.json")).unwrap();
    entries
        .into_iter()
        .find(|entry| entry["capture"]["game"] == game)
        .unwrap()["capture"]
        .take()
}

fn source(root: &Path, capture: &Value) -> std::path::PathBuf {
    let path = root.join(format!("{}.json", capture["game"].as_str().unwrap()));
    fs::write(&path, serde_json::to_vec(capture).unwrap()).unwrap();
    path
}

#[test]
fn sequential_rounds_replay_exact_native_messages_without_reinitializing() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let capture = fixture("rp_1");
    // Hand-checked source: two sequential normal rounds, action counters 0 then 1.
    assert_eq!(capture["rounds"][0]["steps"][0]["request"]["an"], 0);
    assert_eq!(capture["rounds"][1]["steps"][0]["request"]["an"], 1);
    assert_eq!(
        capture["rounds"][0]["steps"][0]["response"]["data"]["binary"],
        "AAAAAgAAAAQAAAAGAAAAPAAAACQAAAAUAAAAAwAAACIAAAFsAAAAAAAAAAI="
    );
    let input = source(root.path(), &capture);
    let output = root.path().join("bank");
    let bank = ruby_import::import(&input, &output)?;
    assert_eq!(bank.attempts.len(), 2);
    for attempt in &bank.attempts {
        let sample = attempt.metadata["sample"].as_u64().unwrap() as usize;
        let round = &capture["rounds"][sample];
        assert_eq!(attempt.state, "complete");
        assert_eq!(attempt.requests, 1);
        assert_eq!(attempt.metadata["provider"], "rubyplay");
        assert_eq!(attempt.metadata["source_format"], "rubyplay_parsed_json");
        assert_eq!(attempt.metadata["source_filename"], "rp_1.json");
        assert_eq!(
            attempt.metadata["source_sha256"],
            capture_runner::model::sha256(&fs::read(&input)?)
        );
        assert_eq!(
            attempt.metadata["source_context"]["init_request"],
            capture["init_request"]
        );
        assert_eq!(attempt.metadata["v_math"], capture["v_math"]);
        let mut replay = ReplaySession::open(&output, attempt)?;
        assert!(replay.initial_events().is_empty()); // attempt_start stays in bank/inspect metadata; no native initial exchange.
        assert_eq!(
            replay.expected_request().unwrap()["data"],
            round["steps"][0]["request"]
        );
        let events = replay
            .submit(&json!({"event":"native_request","data":round["steps"][0]["request"]}))?;
        assert_eq!(events["events"][0]["event"], "native_response");
        assert_eq!(events["events"][0]["data"], round["steps"][0]["response"]);
        assert!(replay.complete());
    }
    let loaded = capture_bank::load(&output)?;
    assert_eq!(loaded.attempts.len(), 2);
    Ok(())
}

#[test]
fn rejected_and_inconsistent_rounds_keep_context_and_remain_partial() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut capture = fixture("rp_120");
    capture["rounds"][0]["steps"][0]["response"]["data"]["next_action"] = json!("freespin");
    let input = source(root.path(), &capture);
    let output = root.path().join("bank");
    let bank = ruby_import::import(&input, &output)?;
    assert_eq!(bank.attempts.len(), 3);
    let first = bank
        .attempts
        .iter()
        .find(|a| a.metadata["sample"] == 0)
        .unwrap();
    assert_eq!(first.state, "partial");
    assert_eq!(first.result.as_ref().unwrap()["status"], "partial");
    assert!(
        first.result.as_ref().unwrap()["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue.as_str().unwrap().contains("closed"))
    );
    let failed = bank
        .attempts
        .iter()
        .find(|a| a.metadata["sample"] == 2)
        .unwrap();
    assert_eq!(failed.state, "partial");
    assert_eq!(failed.requests, 0);
    assert_eq!(failed.result.as_ref().unwrap()["status"], "provider_error");
    assert_eq!(
        failed.metadata["source_context"]["errors"],
        capture["errors"]
    );
    assert_eq!(
        failed.metadata["source_round"]["reason"],
        "rejected_buy_feature"
    );
    assert!(failed.metadata["source_round"].get("steps").is_none());
    let replay = ReplaySession::open(&output, failed)?;
    assert!(replay.exhausted());
    assert!(!replay.complete());
    Ok(())
}

#[test]
fn identity_mismatches_and_malformed_inputs_are_explicit_errors() -> anyhow::Result<()> {
    for target in ["game", "session", "init", "round"] {
        let root = tempfile::tempdir()?;
        let mut capture = fixture("rp_1");
        match target {
            "game" => capture["game"] = json!("rp_999"),
            "session" => capture["session_response"]["funModeData"]["gameId"] = json!(999),
            "init" => capture["init_request"]["funModeData"]["gameId"] = json!(999),
            _ => capture["rounds"][0]["steps"][0]["request"]["funModeData"]["gameId"] = json!(999),
        }
        let input = root.path().join("rp_1.json");
        fs::write(&input, serde_json::to_vec(&capture)?)?;
        let output = root.path().join("bank");
        assert!(ruby_import::import(&input, &output).is_err(), "{target}");
        assert!(!output.exists());
    }
    let root = tempfile::tempdir()?;
    let mut capture = fixture("rp_1");
    capture["rounds"][0]["steps"][0]["response"] = Value::Null;
    let input = source(root.path(), &capture);
    assert!(ruby_import::import(&input, &root.path().join("bank")).is_err());
    fs::write(input, b"{")?;
    assert!(ruby_import::import(root.path(), &root.path().join("bank2")).is_err());
    Ok(())
}

#[test]
fn native_closure_ignores_balances_but_requires_closed_and_accepted_status() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut capture = fixture("rp_1");
    for round in capture["rounds"].as_array_mut().unwrap() {
        round.as_object_mut().unwrap().remove("balance_before");
        round["balance_after"] = json!("unavailable");
        round["steps"][0]["response"]["data"]
            .as_object_mut()
            .unwrap()
            .remove("player");
    }
    let mut rejected = capture["rounds"][0].clone();
    rejected["steps"][0]["response"]["status"] = json!("error");
    capture["rounds"].as_array_mut().unwrap().push(rejected);
    capture["rounds"][1]["closed"] = json!(false);
    let input = source(root.path(), &capture);
    let output = root.path().join("bank");
    let bank = ruby_import::import(&input, &output)?;
    for attempt in &bank.attempts {
        let index = attempt.metadata["sample"].as_u64().unwrap();
        assert_eq!(
            attempt.state,
            if index == 0 { "complete" } else { "partial" }
        );
        if index == 2 {
            assert_eq!(attempt.result.as_ref().unwrap()["status"], "provider_error");
            let mut replay = ReplaySession::open(&output, attempt)?;
            let request = replay.expected_request().cloned().unwrap();
            let released = replay.submit(&request)?;
            assert_eq!(released["events"][0]["data"]["status"], "error");
            assert!(!replay.complete());
        }
    }
    Ok(())
}

#[test]
fn source_tree_is_read_only_and_bad_directory_members_are_not_skipped() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let input = root.path().join("captures");
    fs::create_dir(&input)?;
    source(&input, &fixture("rp_1"));
    assert!(ruby_import::import(&input, &input.join("bank")).is_err());
    assert!(ruby_import::import(&input, &root.path().join("absent/../captures/bank")).is_err());
    assert!(!input.join("bank").exists());
    fs::write(input.join("rp_999.json"), b"{")?;
    let output = root.path().join("bank");
    assert!(ruby_import::import(&input, &output).is_err());
    assert!(!output.exists());
    Ok(())
}

#[test]
fn output_is_never_overwritten() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let input = source(root.path(), &fixture("rp_1"));
    let output = root.path().join("bank");
    ruby_import::import(&input, &output)?;
    let before = fs::read(output.join("index.json"))?;
    assert!(ruby_import::import(&input, &output).is_err());
    assert_eq!(fs::read(output.join("index.json"))?, before);
    Ok(())
}

#[test]
#[ignore = "requires explicit read-only historical evidence path in RUBYPLAY_CAPTURE_DIR"]
fn full_historical_evidence_import() -> anyhow::Result<()> {
    let input = std::env::var_os("RUBYPLAY_CAPTURE_DIR")
        .expect("set RUBYPLAY_CAPTURE_DIR to run evidence import");
    let root = tempfile::tempdir()?;
    let output = root.path().join("bank");
    let bank = ruby_import::import(Path::new(&input), &output)?;
    assert_eq!(
        bank.attempts
            .iter()
            .map(|a| a.metadata["game"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        179
    );
    assert_eq!(bank.attempts.len(), 434);
    assert_eq!(
        bank.attempts
            .iter()
            .filter(|a| a.state == "complete")
            .count(),
        432
    );
    assert_eq!(
        bank.attempts
            .iter()
            .filter(|a| a.state == "partial")
            .count(),
        2
    );
    assert_eq!(bank.attempts.iter().map(|a| a.requests).sum::<u64>(), 1491);
    for attempt in &bank.attempts {
        let mut replay = ReplaySession::open(&output, attempt)?;
        while let Some(request) = replay.expected_request().cloned() {
            replay.submit(&request)?;
        }
        assert_eq!(replay.complete(), attempt.state == "complete");
    }
    Ok(())
}
