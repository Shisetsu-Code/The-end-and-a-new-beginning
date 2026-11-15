//! Strict, ordered local playback of native messages. Never opens a network socket.
use crate::capture_bank::{self, BankAttempt};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::Path,
};

pub struct ReplaySession {
    reader: BufReader<File>,
    sequence: u64,
    pending: Option<Value>,
    initial: Vec<Value>,
    cut_off: Option<String>,
    source_complete: bool,
    terminal_finish: bool,
}

fn comparable(message: &Value) -> Result<Value> {
    let event = message["event"].as_str().context("request event missing")?;
    ensure!(
        capture_bank::outbound(event),
        "expected an outbound request event"
    );
    let mut data = message["data"]
        .as_object()
        .context("request data must be an object")?
        .clone();
    // These fields describe the collector; every actual protocol byte is matched.
    if event != "native_request" {
        data.remove("label");
        data.remove("round_action");
    }
    Ok(json!({"event":event,"data":data}))
}

impl ReplaySession {
    pub fn open(bank: &Path, attempt: &BankAttempt) -> Result<Self> {
        // Verify metadata/results too, so an edited status cannot masquerade as
        // the original captured outcome.
        for name in attempt
            .files
            .keys()
            .filter(|name| name.as_str() != "wire.jsonl")
        {
            capture_bank::open_verified(bank, attempt, name)?;
        }
        let reader = BufReader::new(capture_bank::open_verified(bank, attempt, "wire.jsonl")?);
        let mut session = Self {
            reader,
            sequence: 0,
            pending: None,
            initial: vec![],
            cut_off: None,
            source_complete: attempt.state == "complete",
            terminal_finish: false,
        };
        session.initial = session.advance()?;
        // attempt_start duplicates original attempt metadata, which may contain
        // end-of-round outcomes and later errors in imported historical captures.
        // Preserve it in the bank; automatic replay exposes native initial events.
        session
            .initial
            .retain(|record| record["event"] != "attempt_start");
        Ok(session)
    }

    fn advance(&mut self) -> Result<Vec<Value>> {
        let mut received = Vec::new();
        loop {
            match capture_bank::read_record(&mut self.reader, &mut self.sequence) {
                Ok(Some(record)) => {
                    self.terminal_finish = record["event"] == "attempt_finish"
                        && record["data"]["status"] == "complete";
                    if capture_bank::outbound(record["event"].as_str().unwrap_or_default()) {
                        self.pending = Some(record);
                        break;
                    }
                    received.push(record);
                }
                Ok(None) => {
                    self.pending = None;
                    break;
                }
                Err(error) => {
                    // The bank retains the invalid suffix byte-for-byte. Replay
                    // stops at its boundary and cannot report successful closure.
                    self.cut_off = Some(error.to_string());
                    self.pending = None;
                    break;
                }
            }
        }
        Ok(received)
    }

    pub fn initial_events(&self) -> &[Value] {
        &self.initial
    }
    pub fn expected_request(&self) -> Option<&Value> {
        self.pending.as_ref()
    }
    pub fn exhausted(&self) -> bool {
        self.pending.is_none()
    }
    pub fn complete(&self) -> bool {
        self.exhausted() && self.source_complete && self.terminal_finish && self.cut_off.is_none()
    }
    pub fn cut_off(&self) -> Option<&str> {
        self.cut_off.as_deref()
    }

    /// A mismatch leaves the cursor unchanged. No outcome from another choice,
    /// mode or wager can be substituted for the recorded request.
    pub fn submit(&mut self, request: &Value) -> Result<Value> {
        let expected = self
            .pending
            .as_ref()
            .context("recorded transcript exhausted")?;
        ensure!(
            comparable(expected)? == comparable(request)?,
            "replay request mismatch at recorded sequence {}; no response released",
            expected["seq"]
        );
        let seq = expected["seq"].clone();
        self.pending = None;
        let events = self.advance()?;
        Ok(
            json!({"event":"replay_step","request_seq":seq,"events":events,"exhausted":self.exhausted(),"source_complete":self.source_complete,"cut_off":self.cut_off}),
        )
    }
}

pub fn inspect(bank: &Path, attempt: &BankAttempt) -> Result<Value> {
    let mut session = ReplaySession::open(bank, attempt)?;
    let initial = session.initial_events().to_vec();
    let mut requests = Vec::new();
    while let Some(request) = session.expected_request().cloned() {
        requests
            .push(json!({"seq":request["seq"],"event":request["event"],"data":request["data"]}));
        session.submit(&request)?;
    }
    Ok(
        json!({"id":attempt.id,"metadata":attempt.metadata,"state":attempt.state,"issues":attempt.issues,"initial_events":initial,"requests":requests,"cut_off":session.cut_off()}),
    )
}

pub fn emit_requests(bank: &Path, attempt: &BankAttempt, mut output: impl Write) -> Result<()> {
    let mut session = ReplaySession::open(bank, attempt)?;
    while let Some(request) = session.expected_request().cloned() {
        serde_json::to_writer(
            &mut output,
            &json!({"event":request["event"],"data":request["data"]}),
        )?;
        output.write_all(b"\n")?;
        session.submit(&request)?;
    }
    output.flush()?;
    Ok(())
}

fn active_metadata(metadata: &Value) -> Value {
    let mut projected = serde_json::Map::new();
    for key in [
        "provider",
        "exit",
        "game",
        "mode",
        "sample",
        "version",
        "schema_version",
        "v_protocol",
        "v_math",
        "source_id",
        "source_format",
        "source_filename",
        "source_sha256",
    ] {
        if let Some(value) = metadata
            .get(key)
            .filter(|v| !v.is_object() && !v.is_array())
        {
            projected.insert(key.into(), value.clone());
        }
    }
    Value::Object(projected)
}

pub fn run(
    bank: &Path,
    attempt: &BankAttempt,
    requests: impl BufRead,
    mut output: impl Write,
) -> Result<bool> {
    let mut session = ReplaySession::open(bank, attempt)?;
    fn emit(output: &mut impl Write, value: &Value) -> Result<()> {
        serde_json::to_writer(&mut *output, value)?;
        output.write_all(b"\n")?;
        output.flush()?;
        Ok(())
    }
    emit(
        &mut output,
        &json!({"event":"replay_start","attempt":attempt.id,"metadata":active_metadata(&attempt.metadata),"events":session.initial_events()}),
    )?;
    for (index, line) in requests.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        ensure!(
            line.len() <= capture_bank::MAX_RECORD_BYTES,
            "replay request too large"
        );
        let request: Value = serde_json::from_str(&line)
            .with_context(|| format!("invalid replay request at line {}", index + 1))?;
        let response = session.submit(&request)?;
        emit(&mut output, &response)?;
    }
    emit(
        &mut output,
        &json!({"event":"replay_finish","exhausted":session.exhausted(),"complete":session.complete(),"next_request_seq":session.expected_request().map(|r| &r["seq"]),"cut_off":session.cut_off()}),
    )?;
    Ok(session.complete())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::Journal;
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    #[test]
    fn http_chunks_and_error_attempts_are_replayed_without_financial_fields() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("capture");
        let journal = Journal::create(
            &source,
            &json!({"provider":"fixture","game":"one","mode":"normal"}),
        )?;
        let request = json!({"label":"spin","method":"POST","url":"https://example.invalid/round","headers_base64":[["content-type","YXBwbGljYXRpb24vanNvbg=="]],"body_base64":"eyJiZXQiOjF9","round_action":true});
        let id = journal.record("http_request", &request)?;
        journal.record(
            "http_response_headers",
            &json!({"request_seq":id,"status":200,"headers_base64":[]}),
        )?;
        journal.record(
            "http_response_chunk",
            &json!({"request_seq":id,"body_base64":"AP8="}),
        )?;
        journal.record(
            "http_response_chunk",
            &json!({"request_seq":id,"body_base64":"EIA="}),
        )?;
        journal.record(
            "transport_error",
            &json!({"request_seq":id,"error":"body interrupted"}),
        )?;
        journal.finish(&json!({"status":"transport_error"}))?;
        let bank_path = root.path().join("bank");
        let bank = capture_bank::export(&source, &bank_path)?;
        let mut replay = ReplaySession::open(&bank_path, &bank.attempts[0])?;
        let mut wrong = json!({"event":"http_request","data":request});
        wrong["data"]["body_base64"] = json!("eyJiZXQiOjJ9");
        assert!(replay.submit(&wrong).is_err());
        assert_eq!(replay.expected_request().unwrap()["seq"], 2);
        let result = replay.submit(&json!({"event":"http_request","data":request}))?;
        let bytes: Vec<u8> = result["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["event"] == "http_response_chunk")
            .flat_map(|r| {
                STANDARD
                    .decode(r["data"]["body_base64"].as_str().unwrap())
                    .unwrap()
            })
            .collect();
        assert_eq!(bytes, [0, 255, 16, 128]);
        assert_eq!(result["events"][3]["event"], "transport_error");
        assert!(replay.exhausted());
        assert!(!replay.complete());
        Ok(())
    }

    #[test]
    fn completion_requires_the_recorded_finish_even_if_index_state_is_edited() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("capture");
        let journal = Journal::create(&source, &json!({"game":"one"}))?;
        journal.record("native_request", &json!({"action":"spin"}))?;
        journal.record("native_response", &json!({"next_action":"spin"}))?;
        let path = root.path().join("bank");
        let mut bank = capture_bank::export(&source, &path)?;
        bank.attempts[0].state = "complete".into();
        let mut session = ReplaySession::open(&path, &bank.attempts[0])?;
        session.submit(&json!({"event":"native_request","data":{"action":"spin"}}))?;
        assert!(!session.complete());
        Ok(())
    }
}
