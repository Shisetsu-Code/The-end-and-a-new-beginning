//! Portable, lossless snapshots of capture attempts. No provider connections.
use crate::{journal, model};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};

const FORMAT: &str = "native-capture-bank";
const FILES: [&str; 3] = ["attempt.json", "wire.jsonl", "result.json"];
pub const MAX_RECORD_BYTES: usize = 48 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredFile {
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BankAttempt {
    pub id: String,
    pub source: String,
    pub metadata: Value,
    pub result: Option<Value>,
    pub state: String,
    pub issues: Vec<String>,
    pub events: u64,
    pub requests: u64,
    pub files: BTreeMap<String, StoredFile>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CaptureBank {
    pub format: String,
    pub version: u32,
    pub created_ms: u64,
    pub attempts: Vec<BankAttempt>,
}

pub fn outbound(event: &str) -> bool {
    matches!(
        event,
        "http_request" | "ws_handshake_request" | "ws_send" | "ws_pong" | "native_request"
    )
}

/// Read one complete JSONL record. Preserve a torn tail in the exported file but
/// never interpret it as a response. The limit matches the transport's largest
/// base64 frame plus envelope, without loading an entire campaign into memory.
pub(crate) fn read_record(reader: &mut impl BufRead, previous: &mut u64) -> Result<Option<Value>> {
    let mut bytes = Vec::new();
    let count = reader
        .take((MAX_RECORD_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if count == 0 {
        return Ok(None);
    }
    ensure!(count <= MAX_RECORD_BYTES, "journal record exceeds 48 MiB");
    ensure!(
        bytes.last() == Some(&b'\n'),
        "journal has an unterminated tail after sequence {previous}"
    );
    let record: Value = serde_json::from_slice(&bytes).context("invalid journal JSON")?;
    let seq = record["seq"].as_u64().context("journal sequence missing")?;
    ensure!(
        seq == *previous + 1,
        "journal sequence discontinuity: expected {}, found {seq}",
        *previous + 1
    );
    ensure!(
        record["event"].is_string() && record.get("data").is_some(),
        "journal event/data missing"
    );
    *previous = seq;
    Ok(Some(record))
}

fn attempts_below(input: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut pending = vec![input.to_path_buf()];
    while let Some(dir) = pending.pop() {
        if dir.join("wire.jsonl").exists()
            || dir.join("attempt.json").exists()
            || dir
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("attempt-"))
        {
            found.push(dir);
            continue;
        }
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    found.sort();
    Ok(found)
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn digest_file(file: &mut File) -> Result<StoredFile> {
    let mut digest = Sha256::new();
    let mut count = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        digest.update(&buffer[..n]);
    }
    Ok(StoredFile {
        sha256: hex_digest(digest.finalize()),
        bytes: count,
    })
}

fn copy_snapshot(from: &Path, to: &Path) -> Result<(StoredFile, bool)> {
    ensure!(
        fs::symlink_metadata(from)?.file_type().is_file(),
        "capture file must be regular: {}",
        from.display()
    );
    let source = File::open(from)?;
    let before = source.metadata()?;
    let mut target = journal::private_file(to)?;
    let mut limited = source.take(before.len());
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = limited.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        target.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
        bytes += n as u64;
    }
    target.sync_all()?;
    let after = limited.into_inner().metadata()?;
    let changed = before.len() != bytes
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok();
    Ok((
        StoredFile {
            sha256: hex_digest(hash.finalize()),
            bytes,
        },
        changed,
    ))
}

fn read_json_observation(path: &Path, issues: &mut Vec<String>) -> Option<Value> {
    if !path.exists() {
        return None;
    }
    match File::open(path)
        .map(BufReader::new)
        .map_err(anyhow::Error::from)
        .and_then(|r| serde_json::from_reader(r).map_err(anyhow::Error::from))
    {
        Ok(value) => Some(value),
        Err(error) => {
            issues.push(format!(
                "{}: {error}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
            None
        }
    }
}

/// Export every attempt, including partial/error attempts. The index is published
/// last; copied files remain useful even if the export itself is interrupted.
pub fn export(input: &Path, output: &Path) -> Result<CaptureBank> {
    let input = input
        .canonicalize()
        .context("capture input directory does not exist")?;
    ensure!(input.is_dir(), "capture input must be a directory");
    ensure!(
        !output.exists(),
        "bank output already exists: {}",
        output.display()
    );
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    journal::private_dir(parent)?;
    let destination = parent.canonicalize()?.join(
        output
            .file_name()
            .context("bank output must have a directory name")?,
    );
    ensure!(
        !destination.starts_with(&input),
        "bank output must be outside the source capture tree"
    );
    let sources = attempts_below(&input)?;
    ensure!(!sources.is_empty(), "no capture attempts found");
    create_bank_dir(&destination)?;
    journal::private_dir(&destination.join("attempts"))?;
    let mut bank = CaptureBank {
        format: FORMAT.into(),
        version: 1,
        created_ms: model::now_ms(),
        attempts: vec![],
    };
    for source in sources {
        let relative = source.strip_prefix(&input)?.to_string_lossy();
        let relative = if relative.is_empty() {
            "."
        } else {
            relative.as_ref()
        };
        let id = model::sha256(relative.as_bytes());
        let dir = destination.join("attempts").join(&id);
        journal::private_dir(&dir)?;
        let mut files = BTreeMap::new();
        let mut issues = Vec::new();
        for name in FILES {
            let from = source.join(name);
            if !from.try_exists()? {
                continue;
            }
            let (file, changed) = copy_snapshot(&from, &dir.join(name))?;
            files.insert(name.into(), file);
            if changed {
                issues.push(format!(
                    "{name} changed during export; copied prefix retained"
                ));
            }
        }
        let metadata =
            read_json_observation(&dir.join("attempt.json"), &mut issues).unwrap_or(Value::Null);
        if metadata.is_null() {
            issues.push("attempt metadata missing or invalid".into());
        }
        let result = read_json_observation(&dir.join("result.json"), &mut issues);
        let mut events = 0;
        let mut requests = 0;
        let mut last = Value::Null;
        if files.contains_key("wire.jsonl") {
            let mut reader = BufReader::new(File::open(dir.join("wire.jsonl"))?);
            loop {
                match read_record(&mut reader, &mut events) {
                    Ok(Some(event)) => {
                        requests +=
                            u64::from(outbound(event["event"].as_str().unwrap_or_default()));
                        last = event;
                    }
                    Ok(None) => break,
                    Err(error) => {
                        issues.push(error.to_string());
                        break;
                    }
                }
            }
        } else {
            issues.push("wire journal missing".into());
        }
        let closed = result.as_ref().is_some_and(|r| r["status"] == "complete")
            && last["event"] == "attempt_finish"
            && last["data"]["status"] == "complete";
        let state = if closed && issues.is_empty() {
            "complete"
        } else {
            "partial"
        };
        bank.attempts.push(BankAttempt {
            id,
            source: relative.into(),
            metadata,
            result,
            state: state.into(),
            issues,
            events,
            requests,
            files,
        });
    }
    journal::write_new_json(
        &destination.join("index.json"),
        &serde_json::to_value(&bank)?,
    )?;
    Ok(bank)
}

pub(crate) fn create_bank_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .context("create new bank directory (output must not exist)")?;
    Ok(())
}

pub fn load(path: &Path) -> Result<CaptureBank> {
    let file = File::open(path.join("index.json")).context("bank index missing")?;
    let bank: CaptureBank =
        serde_json::from_reader(BufReader::new(file)).context("invalid bank index")?;
    ensure!(
        bank.format == FORMAT && bank.version == 1,
        "unsupported capture bank format/version"
    );
    let mut ids = std::collections::BTreeSet::new();
    for attempt in &bank.attempts {
        ensure!(
            attempt.id.len() == 64 && attempt.id.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid bank attempt ID"
        );
        ensure!(ids.insert(&attempt.id), "duplicate bank attempt ID");
        ensure!(
            matches!(attempt.state.as_str(), "complete" | "partial"),
            "unknown bank attempt state"
        );
        for name in attempt.files.keys() {
            ensure!(FILES.contains(&name.as_str()), "unknown bank file name");
        }
    }
    Ok(bank)
}

pub fn summary(bank: &CaptureBank) -> Value {
    json!({"format":bank.format,"version":bank.version,"attempts":bank.attempts.len(),
        "complete":bank.attempts.iter().filter(|a| a.state == "complete").count(),
        "partial":bank.attempts.iter().filter(|a| a.state == "partial").count(),
        "requests":bank.attempts.iter().map(|a| a.requests).sum::<u64>()})
}

pub(crate) fn open_verified(bank: &Path, attempt: &BankAttempt, name: &str) -> Result<File> {
    use std::io::{Seek, SeekFrom};
    let expected = attempt
        .files
        .get(name)
        .context("bank file not present in index")?;
    let directory = bank.join("attempts").join(&attempt.id);
    ensure!(
        fs::symlink_metadata(bank.join("attempts"))?.is_dir()
            && fs::symlink_metadata(&directory)?.is_dir(),
        "bank attempt directory must not be a symlink"
    );
    let path = directory.join(name);
    ensure!(
        fs::symlink_metadata(&path)?.file_type().is_file(),
        "bank file must be regular"
    );
    let mut file = File::open(&path)?;
    let found = digest_file(&mut file)?;
    if found.sha256 != expected.sha256 || found.bytes != expected.bytes {
        bail!("SHA-256/size mismatch for bank file {name}");
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(file)
}
