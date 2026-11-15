use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub evidence_root: PathBuf,
    #[serde(default = "default_data_root")]
    pub data_root: PathBuf,
    pub run_id: String,
    #[serde(default = "default_parallel")]
    pub default_parallel_games: usize,
    #[serde(default = "default_samples")]
    pub samples_per_mode: u32,
    #[serde(default = "default_steps")]
    pub max_steps: usize,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub exits: Vec<ExitConfig>,
    #[serde(default)]
    pub wireguard_dir: Option<PathBuf>,
    #[serde(default)]
    pub run_as: Option<String>,
    #[serde(default)]
    pub games: Vec<String>,
    #[serde(default)]
    pub modes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub id: String,
    #[serde(default)]
    pub parallel_games: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExitConfig {
    pub id: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub wireguard_config: Option<PathBuf>,
    #[serde(default)]
    pub parallel_games: BTreeMap<String, usize>,
}
fn default_data_root() -> PathBuf {
    "captures".into()
}
fn default_parallel() -> usize {
    1
}
fn default_samples() -> u32 {
    1
}
fn default_steps() -> usize {
    256
}
fn default_timeout() -> u64 {
    30
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let path = std::fs::canonicalize(path).context("open runner configuration")?;
        let base = path.parent().context("configuration has no parent")?;
        let mut c: Self = serde_json::from_slice(&std::fs::read(&path)?)
            .context("parse runner JSON configuration")?;
        c.evidence_root = resolve(base, &c.evidence_root);
        c.data_root = resolve(base, &c.data_root);
        validate_id(&c.run_id, "run_id")?;
        if c.default_parallel_games == 0
            || c.samples_per_mode == 0
            || c.max_steps == 0
            || c.timeout_seconds == 0
        {
            bail!("parallel games, samples, max steps and timeout must be positive");
        }
        if let Some(user) = &c.run_as {
            validate_user(user)?;
        }
        let mut providers = BTreeSet::new();
        for p in &c.providers {
            validate_id(&p.id, "provider id")?;
            if !providers.insert(&p.id) {
                bail!("duplicate provider {}", p.id);
            }
            if p.parallel_games == Some(0) {
                bail!("provider concurrency must be positive");
            }
        }
        let mut ids = BTreeSet::new();
        for e in &mut c.exits {
            validate_id(&e.id, "exit id")?;
            if !ids.insert(e.id.clone()) {
                bail!("duplicate exit {}", e.id);
            }
            if let Some(p) = &mut e.wireguard_config {
                *p = resolve(base, p);
            }
        }
        if let Some(dir) = &mut c.wireguard_dir {
            *dir = resolve(base, dir);
            let mut files = std::fs::read_dir(&dir)
                .context("read wireguard_dir")?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            files.sort();
            for file in files {
                if file.extension().and_then(|s| s.to_str()) != Some("conf") {
                    continue;
                }
                let meta = std::fs::symlink_metadata(&file)?;
                if !meta.is_file() || meta.file_type().is_symlink() {
                    bail!("WireGuard configs must be regular, non-symlink files");
                }
                let id = file
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .context("WireGuard filename must be UTF-8")?
                    .to_owned();
                validate_id(&id, "WireGuard filename")?;
                if let Some(e) = c.exits.iter_mut().find(|e| e.id == id) {
                    if let Some(explicit) = &e.wireguard_config {
                        if explicit != &file {
                            bail!("exit {id} conflicts with discovered WireGuard config");
                        }
                    } else {
                        e.wireguard_config = Some(file);
                    }
                } else {
                    c.exits.push(ExitConfig {
                        id,
                        namespace: None,
                        wireguard_config: Some(file),
                        parallel_games: BTreeMap::new(),
                    });
                }
            }
        }
        if c.exits.is_empty() {
            bail!(
                "configure explicit exits or a nonempty wireguard_dir; direct networking is never implicit"
            );
        }
        let mut namespaces = BTreeSet::new();
        for e in &c.exits {
            if let Some(ns) = e.namespace_name() {
                validate_namespace(&ns)?;
                if !namespaces.insert(ns) {
                    bail!("exits must use distinct namespaces");
                }
            }
            for (provider, n) in &e.parallel_games {
                validate_id(provider, "concurrency provider")?;
                if *n == 0 {
                    bail!("exit concurrency must be positive");
                }
            }
        }
        Ok(c)
    }
    pub fn parallel_games(&self, exit: &ExitConfig, provider: &str) -> usize {
        exit.parallel_games
            .get(provider)
            .copied()
            .or_else(|| {
                self.providers
                    .iter()
                    .find(|p| p.id == provider)
                    .and_then(|p| p.parallel_games)
            })
            .unwrap_or(self.default_parallel_games)
    }
}
impl ExitConfig {
    pub fn namespace_name(&self) -> Option<String> {
        self.namespace.clone().or_else(|| {
            self.wireguard_config
                .as_ref()
                .map(|_| format!("crwg-{}", short_hash(&self.id)))
        })
    }
}
pub(crate) fn short_hash(value: &str) -> String {
    Sha256::digest(value.as_bytes())[..6]
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect()
}
pub(crate) fn validate_id(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value == "."
        || value == ".."
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        bail!(
            "invalid {label}: use 1–128 ASCII letters, digits, dot, dash or underscore, starting with a letter or digit"
        );
    }
    Ok(())
}
pub(crate) fn validate_namespace(value: &str) -> Result<()> {
    validate_id(value, "namespace")?;
    if !value.starts_with("crwg-") || value.len() > 63 {
        bail!("namespace must start with crwg- and contain at most 63 characters");
    }
    Ok(())
}
pub(crate) fn validate_user(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 32
        || value.starts_with('-')
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        bail!("invalid run_as account name");
    }
    Ok(())
}
fn resolve(base: &Path, path: &Path) -> PathBuf {
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let mut out = PathBuf::new();
    for part in full.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn load(value: serde_json::Value) -> Result<Config> {
        let d = tempfile::tempdir()?;
        let p = d.path().join("runner.json");
        std::fs::write(&p, serde_json::to_vec(&value)?)?;
        Config::load(&p)
    }
    fn base() -> serde_json::Value {
        json!({"evidence_root":"../evidence","run_id":"test-run","exits":[{"id":"direct"}]})
    }
    #[test]
    fn explicit_direct_and_relative_paths() {
        let c = load(base()).unwrap();
        assert!(c.evidence_root.is_absolute());
        assert!(c.data_root.is_absolute());
        assert!(c.exits[0].namespace_name().is_none());
        assert_eq!(c.timeout_seconds, 30);
    }
    #[test]
    fn refuses_implicit_direct() {
        let mut v = base();
        v["exits"] = json!([]);
        assert!(load(v).is_err());
    }
    #[test]
    fn concurrency_exit_then_provider_then_default() {
        let mut v = base();
        v["default_parallel_games"] = json!(2);
        v["providers"] = json!([{"id":"hacksaw","parallel_games":4}]);
        v["exits"][0]["parallel_games"] = json!({"hacksaw":7});
        let c = load(v).unwrap();
        assert_eq!(c.parallel_games(&c.exits[0], "hacksaw"), 7);
        assert_eq!(c.parallel_games(&c.exits[0], "other"), 2);
        let mut e = c.exits[0].clone();
        e.parallel_games.clear();
        assert_eq!(c.parallel_games(&e, "hacksaw"), 4);
    }
    #[test]
    fn discovers_thirty_exits_and_preserves_overrides() {
        let d = tempfile::tempdir().unwrap();
        let wg = d.path().join("wg");
        std::fs::create_dir(&wg).unwrap();
        for i in 0..30 {
            std::fs::write(
                wg.join(format!("exit-{i:02}.conf")),
                "not read at config discovery",
            )
            .unwrap();
        }
        let p = d.path().join("config.json");
        std::fs::write(&p,json!({"run_id":"test","evidence_root":"evidence","wireguard_dir":"wg","exits":[{"id":"exit-00","parallel_games":{"hacksaw":9}}]}).to_string()).unwrap();
        let c = Config::load(&p).unwrap();
        assert_eq!(c.exits.len(), 30);
        assert!(
            c.exits
                .iter()
                .all(|e| e.wireguard_config.as_ref().unwrap().is_absolute())
        );
        assert_eq!(c.parallel_games(&c.exits[0], "hacksaw"), 9);
        assert!(c.exits[0].namespace_name().unwrap().starts_with("crwg-"));
    }
    #[test]
    fn rejects_unsafe_names_zero_limits_and_duplicates() {
        for (key, value) in [
            ("run_id", json!("../bad")),
            ("default_parallel_games", json!(0)),
            ("samples_per_mode", json!(0)),
            ("max_steps", json!(0)),
            ("timeout_seconds", json!(0)),
            ("run_as", json!("-root")),
        ] {
            let mut v = base();
            v[key] = value;
            assert!(load(v).is_err(), "accepted {key}");
        }
        let mut v = base();
        v["exits"] = json!([{"id":"one","namespace":"../../root"}]);
        assert!(load(v).is_err());
        let mut v = base();
        v["exits"] = json!([{"id":"same"},{"id":"same"}]);
        assert!(load(v).is_err());
    }
}
