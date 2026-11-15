//! Native capture connector derived from standalone info/nolimit/codigo/nolimit.rs.
//! Published source SHA-256: 1d48d2aef487cdbf9fc55c92c025f568ed6c8386d09943a9cc5c0afe82b7658a
//! Reference transport: game-runner/src/adapters/nolimit.rs; no runtime dependency.
//! Generic native selectors and continuations ported from nolimit_protocol.py,
//! nolimit_client.py and nolimit_run.py. Public binding provenance is embedded
//! in bindings/nolimit-controls.json; evidence is never loaded at runtime.
use crate::{
    context::{NativeSocket, RunContext, SocketPayload},
    model::{Completion, Game, Mode, decimal},
};
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::Url;
use uuid::Uuid;

const PROTOCOL: &str = "@nolimitcity/game-communication@0.4.1";
const STATIC: &str = "https://demo.nolimitcdn.com";
const OPEN: &str = "https://demo.nolimitcity.com/EjsFrontWeb/fs";
const MAX_DECODED: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct Selector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    feature_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    game_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    side_name: Option<String>,
}

fn bindings() -> &'static Value {
    static BINDINGS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    BINDINGS.get_or_init(|| {
        serde_json::from_str(include_str!("../../bindings/nolimit-controls.json"))
            .expect("reviewed Nolimit public bindings must be valid JSON")
    })
}

pub fn validate(game: &Game, mode: &Mode, _settings: &Value) -> Result<()> {
    let row = &bindings()["games"][&game.id];
    ensure!(
        row.is_object(),
        "Nolimit game has no reviewed public binding"
    );
    ensure!(
        row["name"] == game.name,
        "Nolimit game identity differs from binding"
    );
    let binding = &row["modes"][&mode.key];
    ensure!(
        binding.is_object(),
        "Nolimit requested mode has no reviewed binding"
    );
    ensure!(
        binding["kind"] == serde_json::to_value(mode.kind)?,
        "Nolimit mode kind differs from binding"
    );
    Ok(())
}

fn published_selector(init: &Value, requested: &str) -> Result<Selector> {
    let mut published = BTreeMap::new();
    if init["allowedFeatureBets"].is_object() {
        let modes = init["allowedGameModes"]
            .as_array()
            .ok_or_else(|| crate::model::unknown_event("Nolimit cap modes missing"))?;
        for mode in modes {
            let mode = mode
                .as_str()
                .ok_or_else(|| crate::model::unknown_event("Nolimit cap mode is not text"))?;
            if init["game"]["playedCapMode"] != mode {
                published.insert(
                    format!("{mode}+NORMAL_PLAY"),
                    Selector {
                        game_mode: Some(mode.into()),
                        ..Selector::default()
                    },
                );
            }
            let mut scoped = init.clone();
            scoped["allowedFeatureBets"] = init["allowedFeatureBets"][mode].clone();
            scoped["game"]["featureBuyTimesBetValue"] =
                init["game"]["featureBuyTimesBetValueAll"][mode].clone();
            for (key, mut selector) in plain_selectors(&scoped)? {
                selector.game_mode = Some(mode.into());
                ensure!(
                    published
                        .insert(format!("{mode}+{key}"), selector)
                        .is_none(),
                    "duplicate native control"
                );
            }
        }
    } else {
        published = plain_selectors(init)?;
    }
    published.remove(requested).ok_or_else(|| {
        crate::model::unknown_event(format!(
            "Nolimit requested control no longer published: {requested}"
        ))
    })
}

fn plain_selectors(init: &Value) -> Result<BTreeMap<String, Selector>> {
    let mut result = BTreeMap::new();
    let allowed = init
        .get("allowedFeatureBets")
        .unwrap_or(&init["allowedGameFeatures"]);
    let mut contexts = vec![("NORMAL_PLAY".to_string(), None)];
    if let Some(features) = init["game"]["featureBuyTimesBetValue"].as_array() {
        for feature in features {
            let name = feature["name"]
                .as_str()
                .ok_or_else(|| crate::model::unknown_event("Nolimit feature name missing"))?;
            if feature["type"] == "SIDE_BET"
                || !allowed
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == name))
            {
                continue;
            }
            let selector = Selector {
                feature_name: Some(name.into()),
                ..Selector::default()
            };
            ensure!(
                result.insert(name.into(), selector).is_none(),
                "duplicate native control"
            );
            contexts.push((name.into(), Some(name.into())));
        }
    }
    for (context, feature_name) in contexts {
        if let Some(sides) = init["game"]["sideBetCosts"][&context].as_object() {
            for name in sides.keys() {
                if !init["allowedGameFeatures"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == name))
                {
                    continue;
                }
                let selector = Selector {
                    feature_name: feature_name.clone(),
                    side_name: Some(name.clone()),
                    game_mode: None,
                };
                ensure!(
                    result
                        .insert(format!("{context}+{name}"), selector)
                        .is_none(),
                    "duplicate native side control"
                );
            }
        }
    }
    Ok(result)
}

fn minimum_bet(init: &Value, selector: &Selector) -> Result<Decimal> {
    ensure!(init["init"] == "ok", "native initialization rejected");
    if init["game"]["nextMode"] != "NORMAL" || init["game"]["isRestoreState"] == true {
        return Err(crate::model::unknown_event("not a fresh idle Nolimit demo"));
    }
    ensure!(
        init["gameClientConfiguration"]["operator"] == "FANPAGE_DEMO",
        "native public demo operator not established"
    );
    let cap = selector
        .game_mode
        .as_deref()
        .or_else(|| init["game"]["playedCapMode"].as_str());
    let mut levels = &init["betLevels"];
    if let Some(cap) = cap.filter(|cap| levels.get(*cap).is_some()) {
        levels = &levels[cap];
    }
    let mut available = levels
        .get("availableBetLevels")
        .unwrap_or(&levels["betLevels"]);
    if let Some(feature) = &selector.feature_name {
        let mut features = &init["featureBetLevels"];
        if let Some(cap) = cap.filter(|cap| features.get(*cap).is_some()) {
            features = &features[cap];
        }
        if let Some(feature_levels) = features.get(feature) {
            available = feature_levels;
        }
    }
    let bet = available
        .as_array()
        .context("missing published bet levels")?
        .iter()
        .map(amount)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .min()
        .context("empty published bet levels")?;
    ensure!(bet > Decimal::ZERO, "published bet is not positive");
    Ok(bet)
}

fn entry_request(init: &Value, code: &str, mode_key: &str) -> Result<Value> {
    let selector = if mode_key == "normal" {
        Selector::default()
    } else {
        published_selector(init, mode_key)?
    };
    let expected: Selector =
        serde_json::from_value(bindings()["games"][code]["modes"][mode_key]["selector"].clone())?;
    if selector != expected {
        return Err(crate::model::unknown_event(format!(
            "Nolimit live selector changed for {code}/{mode_key}"
        )));
    }
    let cap = selector
        .game_mode
        .as_deref()
        .or_else(|| init["game"]["playedCapMode"].as_str());
    let expected_info = &bindings()["games"][code]["native_game_info"];
    let expected_name = cap
        .and_then(|cap| expected_info.get(cap))
        .unwrap_or(expected_info)
        .get("displayName");
    let live_info = &init["gameInfo"];
    let live_name = cap
        .and_then(|cap| live_info.get(cap))
        .unwrap_or(live_info)
        .get("displayName");
    if expected_name.is_none() || live_name != expected_name {
        return Err(crate::model::unknown_event(format!(
            "Nolimit native display identity changed for {code}/{mode_key}"
        )));
    }
    let bet = minimum_bet(init, &selector)?;
    let mut request = json!({"type":"normalBet","bet":bet.to_string()});
    let mut interaction = serde_json::Map::new();
    if init["allowedFeatureBets"].is_object() {
        let cap = selector
            .game_mode
            .as_deref()
            .or_else(|| init["game"]["playedCapMode"].as_str())
            .ok_or_else(|| crate::model::unknown_event("Nolimit default cap mode missing"))?;
        if !init["allowedGameModes"]
            .as_array()
            .is_some_and(|modes| modes.iter().any(|m| m == cap))
        {
            return Err(crate::model::unknown_event(
                "Nolimit selected cap mode not offered",
            ));
        }
        interaction.insert("gameMode".into(), json!(cap));
    }
    if let Some(feature) = &selector.feature_name {
        request["type"] = json!("featureBet");
        request["featureName"] = json!(feature);
        interaction.insert("featureName".into(), json!(feature));
    }
    if let Some(side) = &selector.side_name {
        interaction.insert("sideBet".into(), json!(side));
    }
    if code == "TsarWars" && request["type"] == "normalBet" {
        let native = init["game"]["nextGameSelect"]
            .as_i64()
            .filter(|v| (1..=3).contains(v))
            .unwrap_or(1);
        interaction.insert("gameSelect".into(), json!(native));
    }
    if !interaction.is_empty() {
        request["playerInteraction"] = Value::Object(interaction);
    }
    Ok(request)
}

fn extra_field(code: &str) -> Option<&'static str> {
    match code {
        "SixFeetUnder"
        | "GatorHunters2"
        | "Outsourced2"
        | "SoakedBySeamen"
        | "CatfishHunters"
        | "DuckHuntersHappyHour"
        | "PunkRocker3"
        | "Supersized"
        | "TombstoneBegins"
        | "DasxBoot2"
        | "Bizarre"
        | "GatorHunters"
        | "FlightMode"
        | "KillEmAll"
        | "Seamen"
        | "DuckHunters"
        | "HighwayToHell" => Some("playExtraSpin"),
        "DingDongDeath" | "Disorder" | "GoldenShower" | "DeadMenWalking" => {
            Some("playerSelectedExtraSpin")
        }
        "TrueGrit2" => Some("extraSpin"),
        _ => None,
    }
}

#[derive(Debug)]
struct Choice {
    interaction: &'static str,
    options: Vec<String>,
    default: String,
}
fn choice(interaction: &'static str, options: &[&str], default: &str) -> Choice {
    Choice {
        interaction,
        options: options.iter().map(|s| (*s).into()).collect(),
        default: default.into(),
    }
}
// Match Python json.dumps(family, sort_keys=True) exactly so archived node IDs
// continue to identify the same native choice family.
fn choice_node(code: &str, mode: &Value, interaction: &str) -> Result<String> {
    let family = format!(
        "{{\"game\": {}, \"interaction\": {}, \"mode\": {}}}",
        serde_json::to_string(code)?,
        serde_json::to_string(interaction)?,
        serde_json::to_string(mode)?
    );
    Ok(crate::model::sha256(family.as_bytes())[..16].to_string())
}

fn choice_point(game: &Value, code: &str) -> Result<Option<Choice>> {
    let mode = game["nextMode"]
        .as_str()
        .ok_or_else(|| crate::model::unknown_event("Nolimit nextMode missing"))?;
    if code == "BookOfShadows" && mode == "FREESPIN_PICK" {
        return Ok(Some(
            if game["playerChoseRedrawSymbol"].as_bool() == Some(true)
                || game["mode"] == "FREESPIN_PICK"
            {
                choice("selectedRedraw", &["NO"], "NO")
            } else {
                choice("selectedRedraw", &["NO", "REDRAW"], "NO")
            },
        ));
    }
    let fixed = match (code, mode) {
        ("SanQuentin2", "FREESPIN_SETUP1") | ("Beheaded", "SUPER_FREESPIN_END") => {
            Some(vec!["1", "2"])
        }
        ("BrickSnake2000", "GAMBLE_FIFTY_FIFTY") => Some(vec!["0", "1"]),
        ("Loner", "FEATURE_TICK_TICK_BOOM") | ("TsarWars", "FREESPIN_PICK") => {
            Some(vec!["1", "2", "3"])
        }
        (
            "BuffaloHunter" | "Deadwood" | "DragonTribe" | "EastVsWest" | "ElPaso" | "Infectious5"
            | "MiseryMining",
            "FREESPIN_PICK",
        ) => Some(vec!["1", "2"]),
        _ => None,
    };
    if let Some(options) = fixed {
        return Ok(Some(choice("selectedIndex", &options, options[0])));
    }
    if (code == "DDay" && mode == "FS_PICK") || (code == "GaelicGold" && mode == "FREESPIN_PICKS") {
        let field = if code == "DDay" {
            "fsPicks"
        } else {
            "pickedIndexesAfter"
        };
        let values = game[field]
            .as_array()
            .ok_or_else(|| crate::model::unknown_event(format!("Nolimit {field} missing")))?;
        let options: Vec<String> = values
            .iter()
            .enumerate()
            .filter(|(_, value)| {
                if code == "DDay" {
                    value.as_i64().is_some_and(|n| n < 0)
                } else {
                    **value == false || **value == 0 || value.is_null()
                }
            })
            .map(|(i, _)| i.to_string())
            .collect();
        let default = options
            .first()
            .cloned()
            .ok_or_else(|| crate::model::unknown_event("Nolimit pick has no available indices"))?;
        return Ok(Some(Choice {
            interaction: "selectedIndex",
            options,
            default,
        }));
    }
    if code == "BookOfShadows" && mode == "GAMBLE" {
        let mut options = vec!["NO".to_string()];
        if game["numberOfGamblesDone"]
            .as_u64()
            .zip(game["numberOfGamblesAllowed"].as_u64())
            .is_some_and(|(done, allowed)| done < allowed)
        {
            if game["gambleOptions"].get("FiftyFifty").is_some() {
                options.extend(["RED".into(), "BLACK".into()]);
            }
            if game["gambleOptions"].get("FeatureWin").is_some() {
                options.push("FEATURE".into());
            }
        }
        return Ok(Some(Choice {
            interaction: "gambleOption",
            options,
            default: "NO".into(),
        }));
    }
    if code == "Outsourced" && mode == "UPGRADE_BET" {
        return Ok(Some(choice("playUpgradeBet", &["NO", "YES"], "NO")));
    }
    if mode != "EXTRA_SPIN" {
        return Ok(None);
    }
    if let Some(field) = extra_field(code) {
        return Ok(Some(choice(field, &["NO", "YES"], "NO")));
    }
    if code == "Tanked3" || code == "DuckHunters2" {
        let map = game["extraSpinCostMap"].as_object().ok_or_else(|| {
            crate::model::unknown_event("Nolimit extra spin selector map missing")
        })?;
        let mut options = vec!["NO".to_string()];
        if code == "Tanked3" {
            options.extend(map.keys().cloned());
        } else {
            for option in ["HALF", "SAME"] {
                if map.contains_key(option) {
                    options.push(option.into());
                }
            }
        }
        return Ok(Some(Choice {
            interaction: "playExtraSpin",
            options,
            default: "NO".into(),
        }));
    }
    Err(crate::model::unknown_event(format!(
        "Nolimit unverified extra spin contract for {code}"
    )))
}

fn continuation(game: &Value, code: &str, selected: Option<&str>) -> Result<Option<Value>> {
    let mode = game["nextMode"]
        .as_str()
        .ok_or_else(|| crate::model::unknown_event("Nolimit nextMode missing"))?;
    if mode == "NORMAL" {
        return Ok(None);
    }
    let mut request = json!({"type":"zeroBet","bet":"0.00"});
    if let Some(point) = choice_point(game, code)? {
        let selected = selected.unwrap_or(&point.default);
        if !point.options.iter().any(|o| o == selected) {
            return Err(crate::model::unknown_event(format!(
                "Nolimit unpublished choice {code}/{mode}/{selected}"
            )));
        }
        let mut interaction = json!({});
        match point.interaction {
            "selectedIndex" => {
                interaction["selectedIndex"] = if code == "Infectious5" {
                    json!(selected.parse::<u32>()?)
                } else {
                    json!(selected)
                };
            }
            "selectedRedraw" => {
                if selected == "REDRAW" {
                    interaction["selectedRedraw"] = json!("2");
                } else if game["wasFeatureBuy"] != true {
                    let rows = game["playedRowsInBase"].as_i64().ok_or_else(|| {
                        crate::model::unknown_event("Nolimit playedRowsInBase missing")
                    })?;
                    interaction["selectedIndex"] = json!((rows - 2).to_string());
                }
            }
            "gambleOption" => {
                request["type"] = json!("gambleBet");
                interaction["gambleCollected"] = json!(selected == "NO");
                if selected != "NO" {
                    let (option, selection) = match selected {
                        "RED" => ("0", "1"),
                        "BLACK" => ("0", "2"),
                        "FEATURE" => ("1", "-1"),
                        _ => unreachable!(),
                    };
                    interaction["gambleOption"] = json!(option);
                    interaction["gambleSelection"] = json!(selection);
                }
            }
            "playUpgradeBet" => {
                interaction["playUpgradeBet"] = json!(u8::from(selected == "YES"));
            }
            field => {
                interaction[field] = if code == "Disorder" {
                    json!(u8::from(selected == "YES").to_string())
                } else if matches!(code, "GoldenShower" | "DeadMenWalking") {
                    json!(u8::from(selected == "YES"))
                } else if code == "Tanked3" {
                    json!(if selected == "NO" { "nonono" } else { selected })
                } else if extra_field(code).is_some() {
                    json!(selected.to_ascii_lowercase())
                } else {
                    json!(selected)
                };
            }
        }
        if interaction.as_object().is_some_and(|m| !m.is_empty()) {
            request["playerInteraction"] = interaction;
        }
    } else {
        let special_zero = matches!(
            (code, mode),
            (
                "GoldenShower" | "DeadMenWalking" | "Disorder" | "Tanked3",
                "EXTRA_SPIN_CHECK"
            ) | ("Loner", "FEATURE_PIN_THE_WIN" | "FEATURE_SHOOT_SHOOT_DUCK")
                | ("BonusBunnies", "BONUS_GAME")
                | ("ImmortalFruits", "BONUS_WHEEL")
                | ("FireInTheHole", "BONUS")
                | ("EvilGoblins", "DOUBLE_BOMB")
                | ("MiseryMining", "BONUS" | "BONUS_COOL" | "BONUS_SUPER")
                | (
                    "TrueGrit2",
                    "EXTRA_SPIN_NORMAL" | "EXTRA_SPIN_FS" | "EXTRA_SPIN_SFS" | "EXTRA_SPIN_SDFS"
                )
                | ("TsarWars", "DESTRUCTION")
        );
        if matches!(
            (code, mode),
            ("SanQuentin2", "FREESPIN_SETUP2") | ("HomeOfTheBrave", "CAPWIN_GAME")
        ) {
            request["playerInteraction"] = json!({"selectedIndex":"1"});
        } else if mode == "GAMBLE" {
            request = json!({"type":"gambleBet","bet":"0.00","playerInteraction":{"gambleCollected":true}});
        } else if !special_zero
            && !(!["PICK", "SELECT", "CHOICE", "EXTRA", "GAMBLE"]
                .iter()
                .any(|s| mode.contains(s))
                && ["FREESPIN", "RESPIN", "AVALANCHE"]
                    .iter()
                    .any(|s| mode.contains(s)))
        {
            return Err(crate::model::unknown_event(format!(
                "Nolimit unknown continuation {code}/{mode}"
            )));
        }
    }
    if code == "KennethMustDie" {
        let cap = game["playedCapMode"]
            .as_str()
            .ok_or_else(|| crate::model::unknown_event("Nolimit continuation cap mode missing"))?;
        if request.get("playerInteraction").is_none() {
            request["playerInteraction"] = json!({});
        }
        request["playerInteraction"]["gameMode"] = json!(cap);
    }
    Ok(Some(request))
}

fn amount(value: &Value) -> Result<Decimal> {
    let raw = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => bail!("missing native money"),
    };
    let result = decimal(&raw)?;
    ensure!(result >= Decimal::ZERO, "negative native money");
    Ok(result)
}

fn rc4_hex(key: &str, text: &str) -> Result<String> {
    use std::fmt::Write;
    ensure!(!key.is_empty(), "missing native transport key");
    let key = key.as_bytes();
    let mut state = [0u8; 256];
    for (i, value) in state.iter_mut().enumerate() {
        *value = i as u8;
    }
    let mut j = 0usize;
    for i in 0..256 {
        j = (j + state[i] as usize + key[i % key.len()] as usize) % 256;
        state.swap(i, j);
    }
    let (mut i, mut j) = (0usize, 0usize);
    let mut result = String::with_capacity(text.len() * 2);
    for value in text.bytes() {
        i = (i + 1) % 256;
        j = (j + state[i] as usize) % 256;
        state.swap(i, j);
        let encoded = value ^ state[(state[i] as usize + state[j] as usize) % 256];
        write!(&mut result, "{encoded:02x}")?;
    }
    Ok(result)
}

fn lzw_decode(text: &str) -> Result<String> {
    let Some(encoded) = text.strip_prefix("lzw:") else {
        return Ok(text.into());
    };
    let codes = encoded.encode_utf16().collect::<Vec<_>>();
    let first = *codes.first().context("empty native compressed frame")?;
    let mut previous = vec![first];
    let mut output = previous.clone();
    let mut table: Vec<Vec<u16>> = Vec::new();
    for code in codes.into_iter().skip(1) {
        let entry = if code < 256 {
            vec![code]
        } else {
            let index = code as usize - 256;
            if let Some(entry) = table.get(index) {
                entry.clone()
            } else {
                ensure!(
                    index == table.len(),
                    "invalid native compression dictionary reference"
                );
                let mut entry = previous.clone();
                entry.push(previous[0]);
                entry
            }
        };
        ensure!(
            output.len() + entry.len() <= MAX_DECODED,
            "decoded native frame exceeds retained-body limit"
        );
        output.extend_from_slice(&entry);
        let mut next = previous;
        next.push(entry[0]);
        table.push(next);
        previous = entry;
    }
    String::from_utf16(&output).context("invalid native compressed text")
}

fn decode_frame(text: &str) -> Result<Value> {
    let decoded = lzw_decode(text).map_err(|error| {
        crate::model::unknown_event(format!("Nolimit unrecognized native compression: {error}"))
    })?;
    if decoded.len() > MAX_DECODED {
        return Err(crate::model::unknown_event(
            "Nolimit decoded native frame exceeds retained-body limit",
        ));
    }
    let value: Value = serde_json::from_str(&decoded).map_err(|error| {
        crate::model::unknown_event(format!("invalid native frame JSON: {error}"))
    })?;
    if !value.is_object() {
        return Err(crate::model::unknown_event(
            "Nolimit native frame is not an object",
        ));
    }
    Ok(value)
}

struct Client<'a> {
    ctx: &'a RunContext,
    socket: NativeSocket,
    key: String,
    version: String,
    prefix: String,
    number: u64,
    remember: Value,
}
impl Client<'_> {
    async fn call(&mut self, kind: &str, content: Value, round_action: bool) -> Result<Value> {
        if !(matches!(kind, "init" | "normal" | "balance")) {
            return Err(crate::model::unknown_event(
                "unreviewed native request".to_string(),
            ));
        };
        let id = format!("{}-{}", self.prefix, self.number);
        self.number = self
            .number
            .checked_add(1)
            .context("native request counter overflow")?;
        let mut request =
            json!({"type":kind,"content":content,"protocol":PROTOCOL,"id":id,"data":self.remember});
        if kind == "init" {
            request["gameClientVersion"] = json!(self.version);
        }
        let payload = rc4_hex(&self.key, &serde_json::to_string(&request)?)?;
        let remember = &mut self.remember;
        let response = self
            .ctx
            .ws_codec(
                &mut self.socket,
                kind,
                SocketPayload::Text(payload),
                round_action,
                Some(&id),
                |frame| {
                    let SocketPayload::Text(text) = frame else {
                        return Err(crate::model::unknown_event(
                            "unexpected native binary frame".to_string(),
                        ));
                    };
                    let response = decode_frame(text)?;
                    // Native keys can rotate in an unsolicited frame. RunContext
                    // has durably retained that frame before this callback and
                    // still correlates the returned response by request id.
                    if let Some(key) = response["extPlayerKey"].as_str().filter(|s| !s.is_empty()) {
                        remember["extPlayerKey"] = json!(key);
                    }
                    Ok(vec![response])
                },
            )
            .await?;
        ensure!(
            response["error"].is_null()
                || response["error"] == false
                || response["error"] == 0
                || response["error"] == "",
            "native provider rejected request"
        );
        Ok(response)
    }
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    validate(game, mode, settings)?;
    let info = ctx
        .get("version", &format!("{STATIC}/games/{}/info.json", game.id))
        .await?;
    ensure!(info.status == 200, "public game version unavailable");
    let info: Value = serde_json::from_str(&info.body)?;
    let version = info["version"]
        .as_str()
        .context("public client version missing")?;
    if !(Regex::new(r"^[0-9]+(?:\.[0-9]+){1,3}(?:[-+.][A-Za-z0-9][A-Za-z0-9.-]*)?$")?
        .is_match(version))
    {
        return Err(crate::model::unknown_event(
            "unrecognized public version".to_string(),
        ));
    };
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("action", "open_game")
        .append_pair("clientString", "FANPAGE_DEMO")
        .append_pair("language", "en")
        .append_pair("gameCodeString", &format!("{}@desktop", game.id))
        .finish();
    let launch = ctx
        .http(
            "launcher",
            "POST",
            OPEN,
            BTreeMap::from([(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            )]),
            form,
            false,
        )
        .await?;
    ensure!(launch.status == 200, "public demo launch unavailable");
    let launch: Value = serde_json::from_str(&launch.body)?;
    if !(launch["url"] == "https://demo.nolimitcity.com/EjsGameWeb/gs") {
        return Err(crate::model::unknown_event(
            "unrecognized native public demo launch".to_string(),
        ));
    };
    let key = launch["key"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .context("invalid native demo key")?;
    let mut ws_url = Url::parse("wss://demo.nolimitcity.com/EjsGameWeb/ws/game")?;
    ws_url.query_pairs_mut().append_pair("data", key);
    let socket = ctx
        .websocket(ws_url.as_str(), STATIC, None, BTreeMap::new())
        .await?;
    let mut client = Client {
        ctx,
        socket,
        key: key.into(),
        version: version.into(),
        prefix: Uuid::new_v4().simple().to_string()[..10].into(),
        number: 0,
        remember: json!({}),
    };
    let init = client.call("init", json!({"type":"init"}), false).await?;
    ctx.save_session(&json!({"provider":"nolimit","game":game.id,"mode":mode.key,"version":version,"transport_key":key,"request_prefix":client.prefix,"initial_request_number":client.number,"initial_remember":client.remember,"operator":"FANPAGE_DEMO"}))?;
    let request = entry_request(&init, &game.id, &mode.key)?;
    ctx.note(
        "nolimit_entry",
        &json!({"game":game.id,"mode":mode.key,"selector":request}),
    )?;
    let target = settings.get("choice_target");
    if let Some(target) = target {
        ensure!(
            target["option"].is_string()
                && (target["mode"].is_string() || target["node"].is_string()),
            "Nolimit choice_target requires option and mode or node"
        );
    }
    let mut target_used = false;
    let mut choices = Vec::new();
    let mut response = client.call("normal", request, true).await?;
    let mut responses = vec![response.clone()];
    for _ in 0..ctx.max_steps() {
        let native = &response["game"];
        let mut selected = None;
        if let Some(point) = choice_point(native, &game.id)? {
            let node = choice_node(&game.id, &native["nextMode"], point.interaction)?;
            if let Some(target) = target
                .filter(|t| !target_used && (t["mode"] == native["nextMode"] || t["node"] == node))
            {
                selected = target["option"].as_str();
                target_used = true;
            }
            let chosen = selected.unwrap_or(&point.default);
            let observation = json!({"node":node,"mode":native["nextMode"],"interaction":point.interaction,"options":point.options,"selected":chosen});
            ctx.note("nolimit_choice", &observation)?;
            choices.push(observation);
        }
        let Some(next) = continuation(native, &game.id, selected)? else {
            break;
        };
        response = client.call("normal", next, true).await?;
        responses.push(response.clone());
    }
    if !(response["game"]["nextMode"] == "NORMAL") {
        return Err(crate::model::unknown_event(
            "native continuation budget exhausted; original owner retained".to_string(),
        ));
    };
    ensure!(
        target.is_none() || target_used,
        "Nolimit requested choice was not reached in this native round"
    );
    Ok(Completion {
        native_round_id: None,
        details: json!({"version":version,"mode":mode.key,"initialization":init,"responses":responses,"choices":choices,"choice_target_used":target_used}),
    })
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    validate(game, mode, &Value::Null).is_ok()
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn malformed_compression_and_frame_shape_are_unknown_events() {
        for payload in ["lzw:A\u{102}", "[]"] {
            let error = decode_frame(payload).unwrap_err();
            assert!(error.downcast_ref::<crate::model::UnknownEvent>().is_some());
        }
    }
    use crate::model::ModeKind;

    fn fixtures() -> Value {
        serde_json::from_str(include_str!("../../bindings/nolimit-init-fixtures.json")).unwrap()
    }

    #[test]
    fn every_advertised_binding_has_a_published_native_entry_without_wallet() {
        let fixtures = fixtures();
        let games = bindings()["games"].as_object().unwrap();
        assert_eq!(games.len(), 141);
        let mut count = 0;
        for (code, row) in games {
            let init = &fixtures["games"][code];
            assert!(init.get("balance").is_none());
            for (key, binding) in row["modes"].as_object().unwrap() {
                let kind: ModeKind = serde_json::from_value(binding["kind"].clone()).unwrap();
                let mode = Mode {
                    key: key.clone(),
                    kind,
                    label: key.clone(),
                    requires_prerequisites: Value::Null,
                };
                let game = Game {
                    id: code.clone(),
                    name: row["name"].as_str().unwrap().into(),
                    modes: vec![mode.clone()],
                };
                assert!(supports(&game, &mode), "{code}/{key}");
                let request = entry_request(init, code, key)
                    .unwrap_or_else(|e| panic!("{code}/{key}: {e:#}"));
                let selector: Selector =
                    serde_json::from_value(binding["selector"].clone()).unwrap();
                assert_eq!(
                    request["type"],
                    if selector.feature_name.is_some() {
                        "featureBet"
                    } else {
                        "normalBet"
                    }
                );
                if let Some(feature) = selector.feature_name {
                    assert_eq!(request["featureName"], feature);
                    assert_eq!(request["playerInteraction"]["featureName"], feature);
                }
                if let Some(side) = selector.side_name {
                    assert_eq!(request["playerInteraction"]["sideBet"], side);
                }
                if let Some(cap) = selector.game_mode {
                    assert_eq!(request["playerInteraction"]["gameMode"], cap);
                }
                assert!(amount(&request["bet"]).unwrap() > Decimal::ZERO);
                count += 1;
            }
        }
        assert_eq!(count, 1147);
    }

    #[test]
    fn feature_specific_and_cap_scoped_bets_are_preserved() {
        let fixtures = fixtures();
        let mut initial = fixtures["games"]["SixFeetUnder"].clone();
        initial["featureBetLevels"]["FREESPIN_3_SCATTERS"] = json!(["0.70", "0.40"]);
        let request = entry_request(&initial, "SixFeetUnder", "FREESPIN_3_SCATTERS").unwrap();
        assert_eq!(amount(&request["bet"]).unwrap(), Decimal::new(40, 2));
        let cap = entry_request(
            &fixtures["games"]["KennethMustDie"],
            "KennethMustDie",
            "CAP_MODE_10000+NORMAL_PLAY",
        )
        .unwrap();
        assert_eq!(cap["playerInteraction"]["gameMode"], "CAP_MODE_10000");
    }

    #[test]
    fn missing_or_changed_live_selectors_never_fall_back_to_normal() {
        let mut initial = fixtures()["games"]["SixFeetUnder"].clone();
        initial["allowedFeatureBets"] = json!([]);
        assert_unknown(entry_request(&initial, "SixFeetUnder", "FREESPIN_3_SCATTERS").unwrap_err());
        assert_unknown(entry_request(&initial, "SixFeetUnder", "NEW_FEATURE").unwrap_err());
        initial["betLevels"]["availableBetLevels"] = json!([]);
        assert!(entry_request(&initial, "SixFeetUnder", "normal").is_err());
    }

    fn assert_unknown(error: anyhow::Error) {
        assert!(
            error.downcast_ref::<crate::model::UnknownEvent>().is_some(),
            "{error:#}"
        );
    }

    #[test]
    fn every_archived_choice_selector_matches_the_python_reference_contract() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../bindings/nolimit-choice-fixtures.json"))
                .unwrap();
        let cases = fixtures["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 101);
        for case in cases {
            let code = case["code"].as_str().unwrap();
            let selected = case["selected"].as_str().unwrap();
            let point = choice_point(&case["native"], code).unwrap().unwrap();
            assert_eq!(
                choice_node(code, &case["native"]["nextMode"], point.interaction).unwrap(),
                case["archived_choice_node"]
            );
            let actual = continuation(&case["native"], code, Some(selected))
                .unwrap()
                .unwrap();
            assert_eq!(
                actual, case["expected"],
                "{code}/{}/{selected}",
                case["native"]["nextMode"]
            );
        }
    }

    #[test]
    fn choices_use_native_game_specific_types_without_money_fields() {
        for (code, state, option, field, value) in [
            (
                "SixFeetUnder",
                "EXTRA_SPIN",
                "YES",
                "playExtraSpin",
                json!("yes"),
            ),
            (
                "DingDongDeath",
                "EXTRA_SPIN",
                "NO",
                "playerSelectedExtraSpin",
                json!("no"),
            ),
            (
                "Disorder",
                "EXTRA_SPIN",
                "YES",
                "playerSelectedExtraSpin",
                json!("1"),
            ),
            (
                "GoldenShower",
                "EXTRA_SPIN",
                "YES",
                "playerSelectedExtraSpin",
                json!(1),
            ),
            ("TrueGrit2", "EXTRA_SPIN", "NO", "extraSpin", json!("no")),
            (
                "Outsourced",
                "UPGRADE_BET",
                "YES",
                "playUpgradeBet",
                json!(1),
            ),
            (
                "Infectious5",
                "FREESPIN_PICK",
                "2",
                "selectedIndex",
                json!(2),
            ),
            (
                "SanQuentin2",
                "FREESPIN_SETUP1",
                "2",
                "selectedIndex",
                json!("2"),
            ),
            (
                "SanQuentin2",
                "FREESPIN_SETUP2",
                "1",
                "selectedIndex",
                json!("1"),
            ),
            (
                "Beheaded",
                "SUPER_FREESPIN_END",
                "2",
                "selectedIndex",
                json!("2"),
            ),
            (
                "BrickSnake2000",
                "GAMBLE_FIFTY_FIFTY",
                "1",
                "selectedIndex",
                json!("1"),
            ),
            (
                "Loner",
                "FEATURE_TICK_TICK_BOOM",
                "3",
                "selectedIndex",
                json!("3"),
            ),
            (
                "TsarWars",
                "FREESPIN_PICK",
                "3",
                "selectedIndex",
                json!("3"),
            ),
        ] {
            let next = continuation(&json!({"nextMode":state}), code, Some(option))
                .unwrap()
                .unwrap();
            assert_eq!(next["playerInteraction"][field], value, "{code}/{state}");
        }
        let tanked = continuation(
            &json!({"nextMode":"EXTRA_SPIN","extraSpinCostMap":{"large":"9"}}),
            "Tanked3",
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(tanked["playerInteraction"]["playExtraSpin"], "nonono");
        let duck = continuation(
            &json!({"nextMode":"EXTRA_SPIN","extraSpinCostMap":{"HALF":"9","SAME":"99"}}),
            "DuckHunters2",
            Some("SAME"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(duck["playerInteraction"]["playExtraSpin"], "SAME");
    }

    #[test]
    fn redraw_dynamic_picks_gamble_and_cap_continuations() {
        let redraw = continuation(
            &json!({"nextMode":"FREESPIN_PICK","playedRowsInBase":5}),
            "BookOfShadows",
            Some("REDRAW"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(redraw["playerInteraction"]["selectedRedraw"], "2");
        let picked = continuation(
            &json!({"nextMode":"FS_PICK","fsPicks":[1,-1,-1]}),
            "DDay",
            Some("2"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(picked["playerInteraction"]["selectedIndex"], "2");
        let gaelic = continuation(
            &json!({"nextMode":"FREESPIN_PICKS","pickedIndexesAfter":[true,false]}),
            "GaelicGold",
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(gaelic["playerInteraction"]["selectedIndex"], "1");
        let gamble = continuation(&json!({"nextMode":"GAMBLE","numberOfGamblesDone":0,"numberOfGamblesAllowed":2,"gambleOptions":{"FiftyFifty":"10"}}), "BookOfShadows", Some("BLACK")).unwrap().unwrap();
        assert_eq!(gamble["type"], "gambleBet");
        assert_eq!(gamble["playerInteraction"]["gambleSelection"], "2");
        let cap = continuation(
            &json!({"nextMode":"FREESPIN","playedCapMode":"CAP_MODE_10000"}),
            "KennethMustDie",
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(cap["playerInteraction"]["gameMode"], "CAP_MODE_10000");
    }

    #[test]
    fn completion_uses_native_mode_and_unknown_choices_are_typed() {
        assert!(
            continuation(&json!({"nextMode":"NORMAL"}), "SixFeetUnder", None)
                .unwrap()
                .is_none()
        );
        assert!(
            continuation(&json!({"nextMode":"FREESPIN"}), "SixFeetUnder", None)
                .unwrap()
                .is_some()
        );
        assert_unknown(
            continuation(
                &json!({"nextMode":"FREESPIN_PICK_NEW"}),
                "SixFeetUnder",
                None,
            )
            .unwrap_err(),
        );
        assert_unknown(
            continuation(&json!({"nextMode":"EXTRA_SPIN"}), "Unreviewed", None).unwrap_err(),
        );
        assert_unknown(
            continuation(
                &json!({"nextMode":"FREESPIN_PICK"}),
                "Infectious5",
                Some("99"),
            )
            .unwrap_err(),
        );
        assert_unknown(continuation(&json!({}), "SixFeetUnder", None).unwrap_err());
    }

    #[test]
    fn transport_encoding_matches_rc4_reference() {
        assert_eq!(rc4_hex("Key", "Plaintext").unwrap(), "bbf316e8d940af0ad3");
    }
}
