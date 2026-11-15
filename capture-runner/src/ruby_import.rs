//! Offline conversion of historical RubyPlay parsed JSON captures.
//!
//! Native message objects are preserved as parsed JSON, including opaque binary
//! and sessionData strings. These files are not original HTTP wire captures:
//! headers, HTTP status, and collection timestamps cannot be reconstructed.
//! Journal `at_ms` values describe this import, never the historical exchange.
//! Rounds share their original session; retained init/session messages are only
//! metadata, and each replay starts with its actual recorded native request.
use crate::{
    capture_bank::{self, CaptureBank},
    journal::Journal,
    model,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn source_files(input: &Path) -> Result<Vec<PathBuf>> {
    let kind = fs::symlink_metadata(input)
        .with_context(|| format!("read RubyPlay input {}", input.display()))?
        .file_type();
    let mut paths = Vec::new();
    if kind.is_dir() {
        for entry in fs::read_dir(input)? {
            let path = entry?.path();
            if matching_name(&path) {
                paths.push(path);
            }
        }
        paths.sort();
    } else {
        ensure!(
            kind.is_file(),
            "RubyPlay input must be a regular file or directory"
        );
        ensure!(
            matching_name(input),
            "RubyPlay source filename must match rp_*.json"
        );
        paths.push(input.to_owned());
    }
    ensure!(!paths.is_empty(), "no rp_*.json RubyPlay captures found");
    Ok(paths)
}

fn matching_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("rp_") && name.ends_with(".json"))
}

fn check_identity(message: &Value, game_id: u64) -> Result<()> {
    if let Some(state) = message.get("funModeData") {
        ensure!(state.is_object(), "native funModeData must be an object");
        if let Some(found) = state.get("gameId") {
            ensure!(
                found.as_u64() == Some(game_id),
                "RubyPlay game identity mismatch in native funModeData.gameId"
            );
        }
    }
    Ok(())
}

fn validate(source: &Value, path: &Path) -> Result<()> {
    ensure!(source.is_object(), "RubyPlay capture must be an object");
    let game = source["game"]
        .as_str()
        .context("RubyPlay game must be a string")?;
    ensure!(
        path.file_stem().and_then(|stem| stem.to_str()) == Some(game),
        "RubyPlay game identity mismatch with source filename"
    );
    let game_id: u64 = game
        .strip_prefix("rp_")
        .context("RubyPlay game must match rp_<number>")?
        .parse()
        .context("RubyPlay game must match rp_<number>")?;
    for version in ["v_protocol", "v_math"] {
        ensure!(
            source[version].is_number(),
            "RubyPlay {version} must be a number"
        );
    }
    let modes = source["evidence_modes"]
        .as_array()
        .context("RubyPlay evidence_modes must be an array")?;
    ensure!(
        modes.iter().all(Value::is_string),
        "RubyPlay evidence_modes must contain strings"
    );
    for name in ["session_response", "init_request", "init_response"] {
        ensure!(
            source[name].is_object(),
            "RubyPlay {name} must be an object"
        );
        check_identity(&source[name], game_id).with_context(|| name.to_owned())?;
    }
    let errors = source["errors"]
        .as_array()
        .context("RubyPlay errors must be an array")?;
    for error in errors {
        ensure!(error.is_object(), "RubyPlay error must be an object");
        for name in ["request", "body"] {
            if let Some(message) = error.get(name) {
                ensure!(
                    message.is_object(),
                    "RubyPlay error {name} must be an object"
                );
                check_identity(message, game_id).with_context(|| format!("error {name}"))?;
            }
        }
    }
    let rounds = source["rounds"]
        .as_array()
        .context("RubyPlay rounds must be an array")?;
    ensure!(!rounds.is_empty(), "RubyPlay capture has no rounds");
    for (index, round) in rounds.iter().enumerate() {
        ensure!(
            round.is_object(),
            "RubyPlay round {index} must be an object"
        );
        ensure!(
            round["mode"].as_str().is_some_and(|mode| !mode.is_empty()),
            "RubyPlay round {index} mode missing"
        );
        ensure!(
            round["bet"].is_number(),
            "RubyPlay round {index} bet must be a number"
        );
        ensure!(
            round["closed"].is_boolean(),
            "RubyPlay round {index} closed must be a boolean"
        );
        let steps = round["steps"]
            .as_array()
            .with_context(|| format!("RubyPlay round {index} steps must be an array"))?;
        for (step_index, step) in steps.iter().enumerate() {
            ensure!(
                step.is_object(),
                "RubyPlay round {index} step {step_index} must be an object"
            );
            for name in ["request", "response"] {
                ensure!(
                    step[name].is_object(),
                    "RubyPlay round {index} step {step_index} {name} must be an object"
                );
                check_identity(&step[name], game_id)
                    .with_context(|| format!("round {index} step {step_index} {name}"))?;
            }
        }
    }
    Ok(())
}

/// Resolve an output with possibly nonexistent parents without creating them.
fn destination(path: &Path) -> Result<PathBuf> {
    use std::path::Component;
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => {
                resolved.push(other.as_os_str());
                if resolved.try_exists()? {
                    resolved = resolved.canonicalize()?;
                }
            }
        }
    }
    Ok(resolved)
}

/// Import every retained round, including rejected and unfinished attempts.
/// The destination must be new; sources are read-only and no provider is called.
pub fn import(input: &Path, output: &Path) -> Result<CaptureBank> {
    match fs::symlink_metadata(output) {
        Ok(_) => anyhow::bail!("bank output already exists: {}", output.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let files = source_files(input)?;
    if input.is_dir() {
        ensure!(
            !destination(output)?.starts_with(input.canonicalize()?),
            "bank output must be outside the source capture tree"
        );
    }
    // tempfile directories are private. A malformed later file cannot publish a
    // partially imported bank, since export happens only after all validation.
    let staging = tempfile::tempdir().context("create private RubyPlay import staging")?;
    for path in files {
        ensure!(
            fs::symlink_metadata(&path)?.file_type().is_file(),
            "RubyPlay capture must be regular: {}",
            path.display()
        );
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let source: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid RubyPlay JSON in {}", path.display()))?;
        validate(&source, &path)
            .with_context(|| format!("invalid RubyPlay capture {}", path.display()))?;
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("non-UTF-8 RubyPlay filename")?;
        let mut context = source.as_object().unwrap().clone();
        context.remove("rounds");
        for (index, round) in source["rounds"].as_array().unwrap().iter().enumerate() {
            let mut round_fields = round.as_object().unwrap().clone();
            round_fields.remove("steps");
            let metadata = json!({
                "provider":"rubyplay", "game":source["game"], "mode":round["mode"], "sample":index,
                "source_format":"rubyplay_parsed_json", "source_filename":filename,
                "source_sha256":model::sha256(&bytes), "v_protocol":source["v_protocol"], "v_math":source["v_math"],
                "source_context":context, "source_round":round_fields,
                "message_representation":"parsed native JSON; original HTTP wire unavailable",
                "journal_time_basis":"import observation time; not original collection time",
                "session_scope":"sequential source session; init is context only; starting request state preserved"
            });
            let attempt = staging
                .path()
                .join(source["game"].as_str().unwrap())
                .join(format!("attempt-{index:06}"));
            let journal = Journal::create(&attempt, &metadata)?;
            let steps = round["steps"].as_array().unwrap();
            for step in steps {
                journal.record("native_request", &step["request"])?;
                journal.record("native_response", &step["response"])?;
            }
            let final_response = steps.last().map(|step| &step["response"]);
            let terminal = final_response.is_some_and(|reply| {
                reply["status"] == "ok" && reply["data"]["next_action"] == "spin"
            });
            let closed = round["closed"] == true;
            let complete = closed && terminal;
            let provider_error = !complete
                && (steps
                    .iter()
                    .any(|step| step["response"]["status"] == "error")
                    || source["errors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|error| error["stage"] == round["mode"]));
            let mut issues = Vec::new();
            if closed && !terminal {
                issues.push("source closed claim lacks a final accepted next_action=spin response");
            }
            if !closed {
                issues.push("source round is not closed");
            }
            if steps.is_empty() {
                issues.push(
                    "source round contains no native action pairs; errors retained as context",
                );
            }
            journal.finish(&json!({
                "status":if complete { "complete" } else if provider_error { "provider_error" } else { "partial" },
                "source_closed":round["closed"], "native_terminal":terminal, "native_action_pairs":steps.len(),
                "issues":issues, "completion_basis":"source closed=true and final native status=ok with data.next_action=spin"
            }))?;
        }
    }
    capture_bank::export(staging.path(), output)
}
