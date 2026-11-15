//! Read-only evidence catalogue; legacy ready/accounting flags do not gate capture.
use crate::{
    adapters,
    config::Config,
    model::{Game, Mode, Provider, sha256},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::OnceLock,
};
#[derive(Clone)]
pub struct Catalog {
    pub providers: Vec<Provider>,
    pub settings: BTreeMap<(String, String, String), Value>,
    pub fingerprint: String,
    pub sources: Vec<Value>,
}
impl Catalog {
    pub fn load(root: &Path) -> Result<Self> {
        let mut folders = fs::read_dir(root)
            .with_context(|| format!("evidence folder {}", root.display()))?
            .collect::<std::io::Result<Vec<_>>>()?;
        folders.sort_by_key(|d| d.file_name());
        let mut providers = vec![];
        let mut settings = BTreeMap::new();
        let mut sources = vec![];
        for folder in folders {
            let inventory = folder.path().join("inventario.json");
            if !inventory.is_file() {
                continue;
            }
            let bytes = fs::read(&inventory)?;
            let mut provider: Provider = serde_json::from_slice(&bytes)
                .with_context(|| format!("parse {}", inventory.display()))?;
            ensure!(
                folder.file_name().to_string_lossy() == provider.id,
                "provider folder/id mismatch"
            );
            let plan_path = folder.path().join("plan-revisado.json");
            let plan_bytes = fs::read(&plan_path).context("missing evidence plan")?;
            let manifest: Value =
                serde_json::from_slice(&fs::read(folder.path().join("manifest.json"))?)?;
            for (name, data) in [
                ("inventario.json", &bytes),
                ("plan-revisado.json", &plan_bytes),
            ] {
                let entry = manifest["archivos"]
                    .as_array()
                    .context("invalid evidence manifest")?
                    .iter()
                    .find(|r| r["ruta"] == name)
                    .context("file absent from evidence manifest")?;
                let actual = sha256(data);
                let declared = entry["sha256"].as_str().context("missing evidence hash")?;
                ensure!(
                    actual == declared,
                    "evidence changed: {}/{}; update reviewed manifest explicitly",
                    provider.id,
                    name
                );
                sources.push(json!({"provider":provider.id,"file":name,"sha256":actual}));
            }
            let plan: Value = serde_json::from_slice(&plan_bytes)?;
            for case in plan["casos"]
                .as_array()
                .context("invalid evidence plan cases")?
            {
                let game = case["game"].as_str().context("case game missing")?;
                let mode = case["mode"].as_str().context("case mode missing")?;
                let mut native = case.get("settings").cloned().unwrap_or_else(|| json!({}));
                if let Some(map) = native.as_object_mut() {
                    map.remove("executor");
                }
                ensure!(
                    settings
                        .insert((provider.id.clone(), game.into(), mode.into()), native)
                        .is_none(),
                    "duplicate evidence case"
                );
            }
            // This published supplemental inventory supplies two selectors missing from the common inventory.
            if provider.id == "gamesglobal" {
                let supplement = include_bytes!("../bindings/gamesglobal-inventory.json");
                let extra: Provider = serde_json::from_slice(supplement)?;
                for game in &mut provider.games {
                    if game.modes.is_empty()
                        && let Some(found) = extra.games.iter().find(|g| g.id == game.id)
                    {
                        game.modes = found.modes.clone();
                    }
                }
                sources.push(json!({"provider":provider.id,"file":"embedded/gamesglobal-inventory.json","sha256":sha256(supplement)}));
            }
            // Native IDs and selectors from the delivered IntegratedSlots inventory.
            // Add the four new IDs and two new ante modes without rewriting existing evidence.
            if provider.id == "irondog" {
                let supplement = include_bytes!("../bindings/irondog-integrated.json");
                let extra: Value = serde_json::from_slice(supplement)?;
                for (id, row) in extra["games"]
                    .as_object()
                    .context("Iron Dog supplemental games missing")?
                {
                    let mut modes = vec![];
                    for (key, kind) in row["modes"]
                        .as_object()
                        .context("Iron Dog supplemental modes missing")?
                    {
                        modes.push(Mode {
                            key: key.clone(),
                            kind: serde_json::from_value(kind.clone())?,
                            label: key.clone(),
                            requires_prerequisites: json!(false),
                        });
                    }
                    if let Some(game) = provider.games.iter_mut().find(|g| &g.id == id) {
                        for mode in modes {
                            if let Some(existing) = game.modes.iter().find(|m| m.key == mode.key) {
                                ensure!(
                                    existing.kind == mode.kind,
                                    "Iron Dog supplemental mode kind conflicts: {id}/{}",
                                    mode.key
                                );
                            } else {
                                game.modes.push(mode);
                            }
                        }
                    } else {
                        provider.games.push(Game {
                            id: id.clone(),
                            name: row["name"]
                                .as_str()
                                .context("Iron Dog supplemental name missing")?
                                .to_owned(),
                            modes,
                        });
                    }
                }
                sources.push(json!({"provider":provider.id,"file":"embedded/irondog-integrated.json","sha256":sha256(supplement)}));
            }
            let mut game_ids = BTreeSet::new();
            for game in &provider.games {
                ensure!(game_ids.insert(&game.id), "duplicate game");
                let mut mode_ids = BTreeSet::new();
                for mode in &game.modes {
                    ensure!(mode_ids.insert(&mode.key), "duplicate mode");
                }
            }
            providers.push(provider);
        }
        ensure!(!providers.is_empty(), "no provider inventories found");
        let fingerprint = sha256(&serde_json::to_vec(&sources)?);
        Ok(Self {
            providers,
            settings,
            fingerprint,
            sources,
        })
    }
    pub fn selected(&self, config: &Config) -> Result<Vec<Provider>> {
        for exit in &config.exits {
            for provider in exit.parallel_games.keys() {
                ensure!(
                    self.providers.iter().any(|p| &p.id == provider),
                    "unknown provider in exit {} concurrency override: {}",
                    exit.id,
                    provider
                );
            }
        }
        for requested in &config.providers {
            ensure!(
                self.providers.iter().any(|p| p.id == requested.id),
                "unknown provider {}",
                requested.id
            );
        }
        let mut selected = vec![];
        let mut seen_games = BTreeSet::new();
        let mut seen_modes = BTreeSet::new();
        for provider in &self.providers {
            if !config.providers.is_empty() && !config.providers.iter().any(|p| p.id == provider.id)
            {
                continue;
            }
            let mut p = provider.clone();
            p.games.retain(|game| {
                let matches = config.games.is_empty()
                    || config
                        .games
                        .iter()
                        .any(|id| id == &game.id || id == &format!("{}/{}", provider.id, game.id));
                if matches {
                    seen_games.insert(game.id.clone());
                    seen_games.insert(format!("{}/{}", provider.id, game.id));
                }
                matches
            });
            for game in &mut p.games {
                game.modes.retain(|mode| {
                    let matches = config.modes.is_empty() || config.modes.contains(&mode.key);
                    if matches {
                        seen_modes.insert(mode.key.clone());
                    }
                    matches
                });
            }
            selected.push(p);
        }
        for game in &config.games {
            ensure!(
                seen_games.contains(game),
                "game filter matched no selected provider: {game}"
            );
        }
        for mode in &config.modes {
            ensure!(
                seen_modes.contains(mode),
                "mode filter matched no selected game: {mode}"
            );
        }
        Ok(selected)
    }
    pub fn settings_for(&self, provider: &str, game: &Game, mode: &Mode) -> Result<Value> {
        let mut settings = embedded_settings(provider, game)?;
        if let Some(plan) = self
            .settings
            .get(&(provider.into(), game.id.clone(), mode.key.clone()))
            && let (Some(to), Some(from)) = (settings.as_object_mut(), plan.as_object())
        {
            for (k, v) in from {
                to.insert(k.clone(), v.clone());
            }
        }
        Ok(settings)
    }
    pub fn plan(&self, config: &Config, details: bool) -> Result<Value> {
        let providers = self.selected(config)?;
        let mut rows = vec![];
        for p in &providers {
            let mut supported = 0;
            let mut executable_games = 0;
            let mut unknown = vec![];
            for g in &p.games {
                let mut count = 0;
                for m in &g.modes {
                    if adapters::supports(&p.id, g, m) {
                        supported += 1;
                        count += 1;
                    } else if details {
                        unknown.push(json!({"game":g.id,"mode":m.key,"reason":adapters::unsupported_reason(&p.id,g,m)}));
                    }
                }
                if count > 0 {
                    executable_games += 1;
                }
            }
            let identified: usize = p.games.iter().map(|g| g.modes.len()).sum();
            rows.push(json!({"provider":p.id,"games":p.games.len(),"games_with_executable_modes":executable_games,"identified_modes":identified,"executable_modes":supported,"unsupported_modes":identified-supported,"unsupported":if details{json!(unknown)}else{Value::Null}}));
        }
        let lanes:Vec<Value>=config.exits.iter().map(|e|json!({"exit":e.id,"namespace":e.namespace_name(),"providers":providers.iter().map(|p|json!({"id":p.id,"parallel_games":config.parallel_games(e,&p.id)})).collect::<Vec<_>>()})).collect();
        let configured_slots: usize = config
            .exits
            .iter()
            .map(|e| {
                providers
                    .iter()
                    .map(|p| config.parallel_games(e, &p.id))
                    .sum::<usize>()
            })
            .sum();
        let maximum_active_games: usize = config
            .exits
            .iter()
            .map(|e| {
                providers
                    .iter()
                    .map(|p| {
                        let executable = p
                            .games
                            .iter()
                            .filter(|g| g.modes.iter().any(|m| adapters::supports(&p.id, g, m)))
                            .count();
                        config.parallel_games(e, &p.id).min(executable)
                    })
                    .sum::<usize>()
            })
            .sum();
        Ok(
            json!({"run_id":config.run_id,"catalog_sha256":self.fingerprint,"providers":rows,"exits":lanes,"configured_game_slots":configured_slots,"maximum_active_games_for_selection":maximum_active_games,"samples_per_mode":config.samples_per_mode,"network_requests":0}),
        )
    }
}
fn embedded_settings(provider: &str, game: &Game) -> Result<Value> {
    static ENDORPHINA: OnceLock<Value> = OnceLock::new();
    static PETER: OnceLock<Value> = OnceLock::new();
    static PLAYSON: OnceLock<Value> = OnceLock::new();
    Ok(match provider {
        "endorphina" => {
            let rows = ENDORPHINA.get_or_init(|| {
                serde_json::from_slice(include_bytes!("../bindings/endorphina-catalog.json"))
                    .expect("embedded catalogue")
            });
            let row = rows
                .as_array()
                .context("endorphina catalogue")?
                .iter()
                .find(|r| r["id"] == game.id)
                .context("endorphina launch binding absent")?;
            json!({"launch_url":row["play_url"]})
        }
        "peterandsons" => {
            let rows = PETER.get_or_init(|| {
                serde_json::from_slice(include_bytes!("../bindings/peterandsons.json"))
                    .expect("embedded binding")
            });
            let row = rows
                .get(&game.id)
                .context("Peter & Sons native binding absent")?;
            json!({"wallet":row["wallet"],"currency":row["currency"],"verified_modes":row["verified_modes"]})
        }
        "playson" => {
            let rows = PLAYSON.get_or_init(|| {
                serde_json::from_slice(include_bytes!("../bindings/playson.json"))
                    .expect("embedded binding")
            });
            json!({"binding":rows.get(&game.id).context("Playson binding absent")?})
        }
        "playngo" => adapters::playngo::embedded_settings(game)?,
        "elk" if game.id == "propaganda-2" => json!({"game_id":"10276","xdm":"0"}),
        _ => json!({}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evidence_loaded_without_legacy_readiness_filters() {
        let c = Catalog::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../standalone info"))
            .unwrap();
        assert_eq!(c.providers.len(), 35);
        assert_eq!(
            c.providers.iter().map(|p| p.games.len()).sum::<usize>(),
            9878
        );
        assert_eq!(
            c.providers
                .iter()
                .flat_map(|p| &p.games)
                .map(|g| g.modes.len())
                .sum::<usize>(),
            20090
        );
        assert!(
            c.providers
                .iter()
                .flat_map(|p| &p.games)
                .any(|g| g.modes.is_empty())
        );
    }

    #[test]
    fn irondog_supplement_enumerates_all_native_pairs_once_and_preserves_originals() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../standalone info");
        let catalog = Catalog::load(&root).unwrap();
        let provider = catalog
            .providers
            .iter()
            .find(|p| p.id == "irondog")
            .unwrap();
        let original: Provider =
            serde_json::from_slice(&fs::read(root.join("irondog/inventario.json")).unwrap())
                .unwrap();
        let bytes = include_bytes!("../bindings/irondog-integrated.json");
        let extra: Value = serde_json::from_slice(bytes).unwrap();
        let mut source_pairs = 0;
        for (id, row) in extra["games"].as_object().unwrap() {
            let games: Vec<_> = provider.games.iter().filter(|g| &g.id == id).collect();
            assert_eq!(games.len(), 1, "native game {id} must be listed once");
            let game = games[0];
            if !original.games.iter().any(|g| &g.id == id) {
                assert_eq!(game.name, row["name"].as_str().unwrap());
            }
            for (key, kind) in row["modes"].as_object().unwrap() {
                source_pairs += 1;
                let modes: Vec<_> = game.modes.iter().filter(|m| &m.key == key).collect();
                assert_eq!(modes.len(), 1, "native mode {id}/{key} must be listed once");
                assert_eq!(modes[0].kind.as_str(), kind.as_str().unwrap());
                assert!(adapters::supports("irondog", game, modes[0]));
            }
        }
        assert_eq!(source_pairs, 61);
        for old in &original.games {
            let games: Vec<_> = provider.games.iter().filter(|g| g.id == old.id).collect();
            assert_eq!(games.len(), 1);
            let game = games[0];
            assert_eq!(game.name, old.name);
            for old_mode in &old.modes {
                let modes: Vec<_> = game
                    .modes
                    .iter()
                    .filter(|m| m.key == old_mode.key)
                    .collect();
                assert_eq!(modes.len(), 1);
                assert_eq!(
                    serde_json::to_value(modes[0]).unwrap(),
                    serde_json::to_value(old_mode).unwrap()
                );
            }
        }
        assert_eq!(provider.games.len(), 145);
        assert_eq!(
            provider.games.iter().map(|g| g.modes.len()).sum::<usize>(),
            401
        );
        assert_eq!(
            provider
                .games
                .iter()
                .flat_map(|g| g.modes.iter().map(move |m| (g, m)))
                .filter(|(g, m)| adapters::supports("irondog", g, m))
                .count(),
            64
        );
        assert_eq!(
            provider
                .games
                .iter()
                .filter(|g| g.modes.iter().any(|m| adapters::supports("irondog", g, m)))
                .count(),
            19
        );
        assert!(catalog.sources.iter().any(|source| {
            source["provider"] == "irondog"
                && source["file"] == "embedded/irondog-integrated.json"
                && source["sha256"] == sha256(bytes)
        }));
    }
}
