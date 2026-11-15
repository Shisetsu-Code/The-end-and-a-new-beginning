//! Append before interpretation; every attempt has a new private directory.
use crate::model::{now_ms, sha256};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

struct Inner {
    file: File,
    sequence: u64,
}
#[derive(Clone)]
pub struct Journal {
    path: PathBuf,
    inner: Arc<Mutex<Inner>>,
}
pub fn private_dir(path: &Path) -> Result<()> {
    let mut missing = Vec::new();
    let mut current = path;
    while !current.exists() {
        missing.push(current.to_path_buf());
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current || parent.as_os_str().is_empty() {
            break;
        }
        current = parent;
    }
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)?;
    // Persist directory entries as well as file contents before any network intent.
    // Otherwise a VM/kernel crash could erase the attempt name and permit a replay.
    for created in missing.iter().rev() {
        if let Some(parent) = created.parent() {
            sync_directory(parent)?;
        }
        sync_directory(created)?;
    }
    Ok(())
}
fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            path
        })?
        .sync_all()?;
    }
    Ok(())
}
pub fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    Ok(options.open(path)?)
}
pub fn write_new_json(path: &Path, value: &Value) -> Result<()> {
    let mut file = private_file(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}
impl Journal {
    pub fn create(path: &Path, metadata: &Value) -> Result<Self> {
        ensure!(!path.exists(), "attempt already exists: {}", path.display());
        private_dir(path)?;
        let file = private_file(&path.join("wire.jsonl"))?;
        write_new_json(&path.join("attempt.json"), metadata)?;
        let journal = Self {
            path: path.to_owned(),
            inner: Arc::new(Mutex::new(Inner { file, sequence: 0 })),
        };
        journal.record("attempt_start", metadata)?;
        Ok(journal)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn record(&self, event: &str, data: &Value) -> Result<u64> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
        inner.sequence += 1;
        let seq = inner.sequence;
        let record = json!({"seq":seq,"at_ms":now_ms(),"event":event,"data":data});
        serde_json::to_writer(&mut inner.file, &record)?;
        inner.file.write_all(b"\n")?;
        // Each outbound intent and inbound chunk is durable before the next protocol action.
        inner.file.sync_data().context("persist capture journal")?;
        Ok(seq)
    }
    pub fn finish(&self, value: &Value) -> Result<()> {
        self.record("attempt_finish", value)?;
        write_new_json(&self.path.join("result.json"), value)
    }
    pub fn evidence_id(&self) -> String {
        sha256(self.path.to_string_lossy().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_attempt_survives_without_result() {
        let root = tempfile::tempdir().unwrap();
        let p = root.path().join("attempt");
        let j = Journal::create(&p, &json!({"game":"x"})).unwrap();
        j.record("received", &json!({"raw":"{broken"})).unwrap();
        drop(j);
        let rows: Vec<Value> = fs::read_to_string(p.join("wire.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(rows[1]["data"]["raw"], "{broken");
        assert!(!p.join("result.json").exists());
        assert!(Journal::create(&p, &json!({})).is_err());
    }
    #[test]
    fn completed_capture_cannot_be_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let j = Journal::create(&root.path().join("a"), &json!({})).unwrap();
        j.finish(&json!({"status":"complete"})).unwrap();
        assert!(j.finish(&json!({"status":"bad"})).is_err());
    }
}
