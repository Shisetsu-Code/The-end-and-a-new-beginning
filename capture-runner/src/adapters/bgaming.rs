//! Native legacy JSON and Hyperhive JSON-RPC capture from
//! bet-collector/game-runner/src/bgaming.rs (public demo protocol evidence).
//! Reference source SHA-256: 0d0ef9384a58f90366d75be9f4c7c1f39baba413d10d70bd91420c6cc6f8c097
//! Legacy-v2 selectors ported from the archived Python protocol sources listed in
//! bindings/bgaming-legacy-v2.json. Fresh source/schema validation precedes dispatch.
//! No legacy executable/runtime dependency and no wallet acceptance policy.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, now_ms, sha256, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use url::Url;
use uuid::Uuid;
#[path = "bgaming_legacy_extra.rs"]
mod legacy_extra;
#[path = "bgaming_rpc_extra.rs"]
mod rpc_extra;
#[path = "bgaming_source.rs"]
mod source;
const ORIGIN: &str = "https://demo.bgaming-network.com";

fn legacy_binding(game: &str, mode: &Mode) -> bool {
    static BINDINGS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let bindings = BINDINGS.get_or_init(|| {
        serde_json::from_str(include_str!("../../bindings/bgaming-legacy-v2.json"))
            .expect("embedded BGaming bindings")
    });
    bindings["games"][game]["modes"][&mode.key] == mode.kind.as_str()
}
pub fn supports(game: &Game, mode: &Mode) -> bool {
    if legacy_extra::family(&game.id, mode).is_some() {
        return true;
    }
    if legacy_binding(&game.id, mode) {
        return true;
    }
    let expected = match mode.key.as_str() {
        "normal" => ModeKind::Normal,
        "freespin_chance" | "buy_chance" => ModeKind::AnteBet,
        _ => ModeKind::Purchase,
    };
    mode.kind == expected
        && match game.id.as_str() {
            "SweetSamurai" => mode.key == "normal",
            "BookOfHiddenTombs" => matches!(
                mode.key.as_str(),
                "normal" | "freespin_chance" | "freespin_buy:0" | "freespin_buy:1"
            ),
            "MysteryGarden" => matches!(mode.key.as_str(), "normal" | "freespin_buy"),
            "RedHotChilliChickens" => matches!(
                mode.key.as_str(),
                "normal" | "bonus_buy:normal" | "bonus_buy:super"
            ),
            "ChickenFire" => matches!(
                mode.key.as_str(),
                "normal" | "buy_chance" | "buy_bonus" | "buy_super_bonus" | "buy_ultra_bonus"
            ),
            _ => false,
        }
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("BGaming missing {key}"))
}
fn minimum(value: &Value) -> Result<i64> {
    value
        .as_array()
        .context("BGaming native bet menu missing")?
        .iter()
        .map(|v| {
            v.as_i64()
                .filter(|n| *n > 0)
                .context("BGaming invalid native bet")
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .min()
        .context("BGaming empty bet menu")
}
fn trusted(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.host_str().is_some_and(|h| {
            h == "demo.bgaming-network.com" || h.ends_with(".demo.bgaming-network.com")
        })
}
async fn get(ctx: &RunContext, label: &str, url: &str) -> Result<String> {
    let r = ctx.get(label, url).await?;
    ensure!(r.status == 200, "BGaming {label} HTTP {}", r.status);
    Ok(r.body)
}
fn options(html: &str) -> Result<Value> {
    let marker = Regex::new(r"window\.__OPTIONS__\s*=\s*")?
        .find(html)
        .ok_or_else(|| unknown_event("BGaming public options missing"))?;
    Value::deserialize(&mut serde_json::Deserializer::from_str(
        &html[marker.end()..],
    ))
    .map_err(|e| unknown_event(format!("BGaming options JSON: {e}")))
}
async fn launch(ctx: &RunContext, game: &str) -> Result<(Url, Value, String)> {
    let mut url = Url::parse(&format!("{ORIGIN}/play/{game}/FUN"))?;
    for _ in 0..5 {
        ensure!(trusted(&url), "BGaming official demo URL required");
        let r = ctx.get("bgaming_launch", url.as_str()).await?;
        match r.status {
            200 => {
                let config = options(&r.body)?;
                ensure!(
                    config["server_id"] == "demo" && config["identifier"] == game,
                    "BGaming demo identity mismatch"
                );
                return Ok((url, config, r.body));
            }
            301 | 302 | 303 | 307 | 308 => {
                url = url.join(
                    r.headers
                        .get("location")
                        .context("BGaming demo redirect missing")?,
                )?
            }
            _ => bail!("BGaming launch HTTP {}", r.status),
        }
    }
    bail!("BGaming demo redirect limit")
}
fn headers(page: &Url) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("origin".into(), page.origin().ascii_serialization()),
        ("referer".into(), page.as_str().into()),
        ("accept".into(), "application/json".into()),
    ])
}
fn source_pins(game: &str) -> Option<(&'static str, &'static str)> {
    match game {
        "BookOfHiddenTombs" => Some((
            "259e842f245e3b68352722d61fca2b3244a4f546a528e876fa9bb466899f0fd4",
            "8a8de87f02c2532db2ac0cce920f5dbb96059fd1b77efccf093d874e0157b6e6",
        )),
        "MysteryGarden" => Some((
            "1c486acb50bf2149370bdcd6db3f5816471c6f6f46682e0fc0ba0163eeb303e3",
            "75d0830b5396434867f898734cc6511a07a339e1bc98e0d9cf94f68cc757d116",
        )),
        _ => None,
    }
}
async fn legacy_source(ctx: &RunContext, game: &str) -> Result<String> {
    let base = format!("https://cdn.bgaming-network.com/html/{game}/");
    let loader = get(ctx, "bgaming_loader", &format!("{base}loader.js")).await?;
    if let Some((loader_hash, _)) = source_pins(game)
        && sha256(loader.as_bytes()) != loader_hash
    {
        return Err(unknown_event("BGaming loader version changed"));
    }
    let basic = Regex::new(r#""basic"\s*:\s*\{[^}]*\bres\s*:\s*"([A-Za-z0-9_./-]+)""#)?;
    let assets: BTreeSet<_> = Regex::new(r"/assets/[A-Za-z0-9_-]+\.js")?
        .find_iter(&loader)
        .map(|m| m.as_str())
        .collect();
    let route = if let Some(found) = basic.captures(&loader) {
        let version = &found[1];
        if version.starts_with('/') || version.split('/').any(|v| matches!(v, "" | "." | "..")) {
            return Err(unknown_event("BGaming invalid static source route"));
        }
        format!("{version}/bundle.js")
    } else if assets.len() == 1 {
        assets.first().unwrap().trim_start_matches('/').to_owned()
    } else {
        let pattern = Regex::new(r#"res:[A-Za-z_$][\w$]*="([A-Za-z0-9_.-]+)""#)?;
        let found = pattern
            .captures(&loader)
            .ok_or_else(|| unknown_event("BGaming native bundle path changed"))?;
        format!("{}/bundle.js", &found[1])
    };
    let source = get(ctx, "bgaming_native_source", &format!("{base}{route}")).await?;
    if let Some((_, source_hash)) = source_pins(game)
        && sha256(source.as_bytes()) != source_hash
    {
        return Err(unknown_event("BGaming native client version changed"));
    }
    Ok(source)
}
fn legacy_next(reply: &Value) -> Result<Option<&'static str>> {
    let flow = &reply["flow"];
    if flow["state"] == "closed" {
        return Ok(None);
    }
    let actions = flow["available_actions"]
        .as_array()
        .ok_or_else(|| unknown_event("BGaming continuation actions missing"))?;
    let actions: Vec<_> = actions.iter().filter(|v| *v != "init").collect();
    match actions.as_slice() {
        [action] if *action == "freespin" => Ok(Some("freespin")),
        [action] if *action == "respin" => Ok(Some("respin")),
        _ => Err(unknown_event(format!(
            "BGaming unrecognized continuation: {flow}"
        ))),
    }
}
fn legacy_entry(initial: &Value, mode: &Mode) -> Result<Value> {
    let mut fields = json!({"bet":minimum(&initial["options"]["available_bets"])?});
    if mode.key != "normal" {
        let (name, level) = mode.key.split_once(':').unwrap_or((&mode.key, ""));
        let offered = &initial["options"]["feature_options"];
        if offered["disabled_features"]
            .as_array()
            .is_some_and(|v| v.iter().any(|v| v == name))
        {
            return Err(unknown_event("BGaming mode disabled"));
        }
        // Presence in the native menu establishes availability; its price is not audited.
        let offer = &offered["feature_multipliers"][name];
        if offer.is_null() || offer == &json!(false) {
            return Err(unknown_event("BGaming mode not offered"));
        }
        if level.is_empty() && offer.is_object() {
            return Err(unknown_event("BGaming purchase requires a native tier"));
        }
        if !level.is_empty() {
            if offer[level].is_null() {
                return Err(unknown_event("BGaming feature level not offered"));
            }
            fields["purchased_feature_level"] = json!(level);
        }
        fields["purchased_feature"] = json!(name);
    }
    Ok(fields)
}
struct LegacyPlan {
    entry: Value,
    continuation_options: BTreeMap<&'static str, Value>,
}
fn require_source(condition: bool, detail: &str) -> Result<()> {
    if !condition {
        return Err(unknown_event(format!(
            "BGaming source selector not proved: {detail}"
        )));
    }
    Ok(())
}
fn free_types(source: &str) -> Result<Vec<String>> {
    require_source(
        source.contains("delete this.additionalSpinOptions.freespins_type"),
        "free-spin type lifecycle",
    )?;
    let assignments = Regex::new(r"additionalSpinOptions\.freespins_type=([^,;]+)")?;
    let ternary =
        Regex::new(r"^3===this\.freespinsReelsCount\?([A-Za-z_$][\w$]*):([A-Za-z_$][\w$]*)$")?;
    let identifier = Regex::new(r"^[A-Za-z_$][\w$]*$")?;
    let mut result = Vec::new();
    for assignment in assignments.captures_iter(source) {
        let variables = if let Some(found) = ternary.captures(&assignment[1]) {
            vec![found[1].to_owned(), found[2].to_owned()]
        } else {
            require_source(
                identifier.is_match(&assignment[1]),
                "native free-spin enum expression",
            )?;
            vec![assignment[1].to_owned()]
        };
        for var in variables {
            let literal = Regex::new(&format!(
                r#"(?:^|[^\w$]){}="([a-z_]+)""#,
                regex::escape(&var)
            ))?;
            let values: BTreeSet<String> = literal
                .captures_iter(source)
                .map(|c| c[1].to_owned())
                .collect();
            require_source(values.len() == 1, "unique source-bound free-spin enum")?;
            let value = values.first().unwrap();
            if !result.contains(value) {
                result.push(value.clone());
            }
        }
    }
    require_source(!result.is_empty(), "native free-spin assignments")?;
    Ok(result)
}

fn legacy_plan(initial: &Value, mode: &Mode, source: &str, game: &str) -> Result<LegacyPlan> {
    let mut local = initial.clone();
    let mut key = mode.key.as_str();
    let mut paid = json!({});
    let mut continuation = json!({});
    let mut free = json!({});
    if let Some((base, row)) = key.split_once("@rows:") {
        require_source(
            row.bytes().all(|b| b.is_ascii_digit()) && !row.is_empty(),
            "rows integer",
        )?;
        key = base;
        if game == "BigAtlantisFrenzy" {
            require_source(
                source.contains("this.additionalSpinOptions.rows=this.reelSize")
                    && source.contains("initialAvailableBets[this.reelSize-"),
                "indexed rows",
            )?;
            let value: usize = row.parse()?;
            let index = Regex::new(r"initialAvailableBets\[this.reelSize-([A-Za-z_$][\w$]*)\]")?;
            let minimum_var = index
                .captures(source)
                .ok_or_else(|| unknown_event("BGaming indexed row base not proved"))?;
            let constants = Regex::new(&format!(
                r"(?:^|[^\w$]){}=(\d+)(?:[^\w$]|$)",
                regex::escape(&minimum_var[1])
            ))?;
            let values: BTreeSet<usize> = constants
                .captures_iter(source)
                .filter_map(|c| c[1].parse().ok())
                .collect();
            require_source(values.len() == 1, "unique indexed minimum")?;
            let maximum_var = Regex::new(r"this\.reelSize=([A-Za-z_$][\w$]*)[,;]")?
                .captures(source)
                .ok_or_else(|| unknown_event("BGaming indexed row maximum missing"))?;
            let maximum = Regex::new(&format!(
                r"(?:^|[^\w$]){}=(\d+)(?:[^\w$]|$)",
                regex::escape(&maximum_var[1])
            ))?;
            let maxima: BTreeSet<usize> = maximum
                .captures_iter(source)
                .filter_map(|c| c[1].parse().ok())
                .collect();
            require_source(maxima.len() == 1, "unique indexed maximum")?;
            let table_var=Regex::new(r"changeReelsSize\([^)]*\)\{var [A-Za-z_$][\w$]*=([A-Za-z_$][\w$]*)\[this\.reelSize\]")?.captures(source).ok_or_else(||unknown_event("BGaming indexed source table missing"))?;
            let table = Regex::new(&format!(
                r"(?:^|[^\w$]){}=\{{((?:\d+:\d+)(?:,\d+:\d+)*)\}}",
                regex::escape(&table_var[1])
            ))?;
            let tables: BTreeSet<String> = table
                .captures_iter(source)
                .map(|c| c[1].to_owned())
                .collect();
            require_source(tables.len() == 1, "unique indexed source table")?;
            let keys: BTreeSet<usize> = tables
                .first()
                .unwrap()
                .split(',')
                .filter_map(|v| v.split_once(':').and_then(|(k, _)| k.parse().ok()))
                .collect();
            let minimum_value = *values.first().unwrap();
            let maximum_value = *maxima.first().unwrap();
            require_source(
                minimum_value <= maximum_value
                    && maximum_value.saturating_sub(minimum_value) < 100
                    && keys.iter().copied().eq(minimum_value..=maximum_value)
                    && keys.contains(&value),
                "indexed row domain",
            )?;
            let offset = value
                .checked_sub(*values.first().unwrap())
                .ok_or_else(|| unknown_event("BGaming row below native index base"))?;
            let bet = initial["options"]["available_bets"]
                .get(offset)
                .and_then(Value::as_i64)
                .filter(|v| *v > 0)
                .ok_or_else(|| unknown_event("BGaming indexed minimum missing"))?;
            local["options"]["available_bets"] = json!([bet]);
            paid["rows"] = json!(value);
            continuation["rows"] = json!(value);
        } else {
            require_source(
                source.contains("this.additionalSpinOptions.rows=String(this.reelSize)"),
                "string rows",
            )?;
            let bets = &initial["options"]["valid_bets"][row];
            minimum(bets).map_err(|e| unknown_event(e.to_string()))?;
            local["options"]["available_bets"] = bets.clone();
            if key != "normal" {
                require_source(
                    source.contains("purchased_feature_level=\"\"+this.reelSize")
                        && source.contains("this.FREESPIN_BUY,this.reelSize"),
                    "rows purchase level",
                )?;
                require_source(
                    key.split_once(':').is_some_and(|(_, level)| level == row),
                    "matching row tier",
                )?;
            }
            paid["rows"] = json!(row);
            continuation["rows"] = json!(row);
        }
    }
    if let Some((base, value)) = key.split_once("@base_volatility:") {
        require_source(matches!(value,"low"|"medium") && source.contains("additionalSpinOptions.volatility=1==this.getCurrentVolatility()?\"low\":\"medium\"") && source.contains("delete this.additionalSpinOptions.volatility") && source.contains("this.fsModeSwitcher(Number("), "volatility/free-spin removal")?;
        key = base;
        paid["volatility"] = json!(value);
    }
    if let Some(value) = key.strip_prefix("lines:") {
        require_source(
            source.contains("this.additionalSpinOptions.mode=this.linesCount")
                && !source.contains("delete this.additionalSpinOptions.mode"),
            "line-count lifecycle",
        )?;
        let pattern =
            Regex::new(r"setBetsVisibleMultiplier\(([A-Za-z_$][\w$]*)\[[A-Za-z_$][\w$]*\],!0\)")?;
        let names: BTreeSet<_> = pattern
            .captures_iter(source)
            .map(|c| c[1].to_owned())
            .collect();
        require_source(names.len() == 1, "unique line factor table")?;
        let name = names.first().unwrap();
        let table = Regex::new(&format!(
            r"(?:^|[^\w$]){}=\{{((?:\d+:\d+)(?:,\d+:\d+)*)\}}",
            regex::escape(name)
        ))?;
        let tables: BTreeSet<_> = table
            .captures_iter(source)
            .map(|c| c[1].to_owned())
            .collect();
        require_source(
            tables.len() == 1
                && tables
                    .first()
                    .unwrap()
                    .split(',')
                    .any(|v| v.split_once(':').is_some_and(|(k, _)| k == value)),
            "active native line table",
        )?;
        let arrays = Regex::new(r#"=\[((?:"\d+")(?:,"\d+")+)\]"#)?;
        require_source(
            arrays
                .captures_iter(source)
                .any(|c| c[1].split(',').any(|v| v == format!("\"{value}\""))),
            "native line string array",
        )?;
        key = "normal";
        paid["mode"] = json!(value);
        continuation["mode"] = json!(value);
    }
    if let Some(rest) = key.strip_prefix("gold_symbols:") {
        let (level, base) = rest
            .split_once('/')
            .ok_or_else(|| unknown_event("BGaming gold selector missing"))?;
        require_source(
            matches!(level, "1" | "2" | "3" | "4" | "5")
                && Regex::new(r#"additionalSpinOptions\.gold_symbols_count=""\+[A-Za-z_$][\w$]*"#)?
                    .is_match(source)
                && source.contains("this.BET_BY_SPECIAL_LVL[this.goldSymbolsLevel]")
                && source.contains("delete this.additionalSpinOptions.gold_symbols_count"),
            "gold symbols lifecycle",
        )?;
        let vars = Regex::new(r"BET_BY_SPECIAL_LVL=([A-Za-z_$][\w$]*)")?;
        let names: BTreeSet<String> = vars
            .captures_iter(source)
            .map(|c| c[1].to_owned())
            .collect();
        require_source(!names.is_empty(), "native gold domain variable")?;
        let mut gold_domain = None;
        for name in names {
            let table = Regex::new(&format!(
                r"(?:^|[^\w$]){}=\{{((?:\d+:\d+)(?:,\d+:\d+)*)\}}",
                regex::escape(&name)
            ))?;
            let tables: BTreeSet<String> = table
                .captures_iter(source)
                .map(|c| c[1].to_owned())
                .collect();
            require_source(tables.len() == 1, "unique native gold domain table")?;
            let domain = tables.first().unwrap().clone();
            require_source(
                gold_domain.as_ref().is_none_or(|v| v == &domain),
                "matching gold domain tables",
            )?;
            let keys: BTreeSet<_> = domain
                .split(',')
                .filter_map(|v| v.split_once(':').map(|(k, _)| k))
                .collect();
            require_source(
                keys == BTreeSet::from(["1", "2", "3", "4", "5"]),
                "native gold selector domain",
            )?;
            gold_domain = Some(domain);
        }
        paid["gold_symbols_count"] = json!(level);
        // The active server menu carries the purchase tier for this gold level.
        key = base;
        if key != "normal" {
            let key_owned = format!("{key}:{level}");
            let m = Mode {
                key: key_owned,
                kind: mode.kind,
                label: String::new(),
                requires_prerequisites: Value::Null,
            };
            let mut entry = legacy_entry(&local, &m)?;
            entry
                .as_object_mut()
                .unwrap()
                .extend(paid.as_object().unwrap().clone());
            return Ok(LegacyPlan {
                entry,
                continuation_options: BTreeMap::new(),
            });
        }
    }
    if let Some((base, value)) = key.split_once("/type:") {
        require_source(
            free_types(source)?.iter().any(|v| v == value),
            "requested source-bound free-spin type",
        )?;
        key = base;
        free["freespins_type"] = json!(value);
    } else if matches!(
        game,
        "MonsterHunt" | "AlohaKingElvis" | "CatchTheGoldHoldAndWin"
    ) {
        free["freespins_type"] = json!(free_types(source)?[0]);
    }
    if let Some((base, portal)) = key.split_once("/portal:") {
        require_source(
            game == "FrenzyClusters"
                && matches!(portal, "0" | "1" | "2" | "3" | "4")
                && source.contains("selected_portal"),
            "Frenzy portal",
        )?;
        key = base;
        free["selected_portal"] = json!(portal.parse::<u8>()?);
    }
    if game == "FrenzyClusters" && free.get("selected_portal").is_none() {
        // run_demo_qa.py settle uses deterministic portal 0 for a natural bonus.
        // Explicit /portal:N modes above override that source-defined policy.
        require_source(
            source.contains("selected_portal"),
            "default Frenzy portal envelope",
        )?;
        free["selected_portal"] = json!(0);
    }
    require_source(!key.contains(['@', '/']), "unparsed selector suffix")?;
    let m = Mode {
        key: key.into(),
        kind: mode.kind,
        label: String::new(),
        requires_prerequisites: Value::Null,
    };
    let mut entry = legacy_entry(&local, &m).map_err(|e| unknown_event(e.to_string()))?;
    entry
        .as_object_mut()
        .unwrap()
        .extend(paid.as_object().unwrap().clone());
    let mut continuation_options = BTreeMap::new();
    if !continuation.as_object().unwrap().is_empty() {
        for command in ["freespin", "respin", "play_bonus"] {
            continuation_options.insert(command, continuation.clone());
        }
    }
    if let Some(portal) = free.get("selected_portal") {
        continuation_options.insert("gamble_bonus", json!({"selected_portal":portal}));
    }
    if let Some(value) = free.get("freespins_type") {
        let mut options = continuation.clone();
        options["freespins_type"] = value.clone();
        continuation_options.insert("freespin", options);
    }
    Ok(LegacyPlan {
        entry,
        continuation_options,
    })
}
fn confirm_purchase(reply: &Value, fields: &Value) -> Result<()> {
    if let Some(selected) = fields.get("freespins_type") {
        require_source(
            reply["features"]["freespins_type"] == *selected,
            "native free-spin type confirmation",
        )?;
    }
    if let Some(feature) = fields.get("purchased_feature") {
        let actual = &reply["flow"]["purchased_feature"];
        require_source(
            actual["name"] == *feature,
            "native purchased feature confirmation",
        )?;
        if let Some(level) = fields.get("purchased_feature_level") {
            let actual = native_id(&actual["level"]);
            require_source(
                actual.as_deref() == level.as_str(),
                "native purchase tier confirmation",
            )?;
        }
    }
    Ok(())
}
fn planned_legacy_next(
    reply: &Value,
    plan: &LegacyPlan,
    source: &str,
    game: &str,
) -> Result<Option<&'static str>> {
    if reply["flow"]["state"] == "closed" {
        return Ok(None);
    }
    let available = reply["flow"]["available_actions"]
        .as_array()
        .ok_or_else(|| unknown_event("BGaming continuation actions missing"))?;
    require_source(
        available.iter().all(Value::is_string),
        "native continuation action type",
    )?;
    let actions: Vec<_> = available
        .iter()
        .filter_map(Value::as_str)
        .filter(|v| *v != "init")
        .collect();
    // An optional extra purchase is not automatically accepted; the free-spin branch is explicit.
    if actions.len() == 2 && actions.contains(&"freespin") && actions.contains(&"buy_extra_bonus") {
        return Ok(Some("freespin"));
    }
    match actions.as_slice() {
        ["gamble_bonus"]
            if game == "FrenzyClusters"
                && plan.continuation_options.contains_key("gamble_bonus") =>
        {
            Ok(Some("gamble_bonus"))
        }
        ["play_bonus"] if game != "CatsSoup" => Ok(Some("play_bonus")),
        ["preselection_game"] => {
            require_source(
                Regex::new(r#"\{command:"preselection_game"\};[A-Za-z_$][\w$]*\.sendCommand\("#)?
                    .is_match(source),
                "preselection request",
            )?;
            Ok(Some("preselection_game"))
        }
        _ => legacy_next(reply),
    }
}

async fn legacy(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    page: Url,
    config: Value,
) -> Result<Completion> {
    let api = Url::parse(text(&config, "api")?)?;
    ensure!(trusted(&api), "BGaming API outside public demo");
    let csrf = text(&config, "csrfTokenHeaderName")?;
    ensure!(
        csrf.to_ascii_lowercase().starts_with("x-")
            && csrf.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "BGaming invalid CSRF header"
    );
    let mut headers = headers(&page);
    headers.insert(csrf.into(), text(&config, "csrfTokenHeaderValue")?.into());
    let series = now_ms();
    let initial = ctx
        .json(
            "bgaming_init",
            api.as_str(),
            &json!({"command":"init","extra_data":{"round_series_id":series}}),
            headers.clone(),
            false,
        )
        .await?;
    if initial["api_version"] != "2" {
        return Err(unknown_event("BGaming unsupported native API version"));
    }
    require_source(
        initial["flow"]["state"] == "ready"
            && initial["flow"]["command"] == "init"
            && initial["flow"]["available_actions"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == "spin")),
        "idle API2 session with advertised spin",
    )?;
    let source = legacy_source(ctx, &game.id).await?;
    let plan = legacy_plan(&initial, mode, &source, &game.id)?;
    let fields = plan.entry.clone();
    ctx.save_session(&json!({"provider":"bgaming","game":game.id,"api":api.as_str(),"round_series_id":series,"csrf_header":csrf}))?;
    let mut command = "spin";
    let mut round = None;
    let mut free_type_confirmed = false;
    for step in 0..ctx.max_steps().min(500) {
        let mut request = json!({"command":command,"extra_data":{"round_series_id":series}});
        if step == 0 {
            request["options"] = fields.clone();
        } else if let Some(options) = plan.continuation_options.get(command) {
            request["options"] = options.clone();
        }
        let reply = ctx
            .json(command, api.as_str(), &request, headers.clone(), true)
            .await?;
        require_source(
            reply["flow"]["command"] == command,
            "native command correlation",
        )?;
        if let Some(id) = native_id(&reply["flow"]["round_id"]) {
            ensure!(
                round.as_ref().is_none_or(|known| *known == id),
                "BGaming native round identity changed"
            );
            round.get_or_insert(id);
        }
        if step == 0 {
            confirm_purchase(&reply, &fields)?;
        }
        if command == "freespin"
            && !free_type_confirmed
            && let Some(options) = plan.continuation_options.get("freespin")
            && options.get("freespins_type").is_some()
        {
            confirm_purchase(&reply, options)?;
            free_type_confirmed = true;
        }
        match planned_legacy_next(&reply, &plan, &source, &game.id)? {
            None => {
                return Ok(Completion {
                    native_round_id: round,
                    details: json!({"family":"legacy","steps":step+1,"mode":mode.key}),
                });
            }
            Some(next) => command = next,
        }
    }
    Err(unknown_event("BGaming continuation budget exhausted"))
}
fn native_id(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
fn rpc_result(envelope: Value, id: &str) -> Result<Value> {
    ensure!(
        envelope["error"].is_null(),
        "BGaming JSON-RPC rejected native request"
    );
    if envelope["jsonrpc"] != "2.0" || envelope["id"] != id {
        return Err(unknown_event(
            "BGaming JSON-RPC response correlation/version changed",
        ));
    }
    envelope
        .get("result")
        .cloned()
        .filter(Value::is_object)
        .ok_or_else(|| unknown_event("BGaming JSON-RPC result missing"))
}
fn hyper_entry(initial: &Value, mode: &Mode, chicken: bool) -> Result<Value> {
    let bet = minimum(&initial["config"]["bet_limits"])?;
    let mut request = json!({"bet":bet,"bet_type":if chicken{"bet"}else{"betting"}});
    if mode.key == "normal" {
        if !chicken {
            request["action"] = json!("spin");
        }
        return Ok(request);
    }
    let feature = if chicken {
        match mode.key.as_str() {
            "buy_chance" => "buy_chance",
            "buy_bonus" | "buy_super_bonus" | "buy_ultra_bonus" => "buy_bonus",
            _ => bail!("BGaming unsupported chicken selector"),
        }
    } else {
        "bonus_buy"
    };
    ensure!(
        initial["config"]["purchased_features"]
            .as_array()
            .is_some_and(|v| v.iter().any(|v| v == feature)),
        "BGaming purchased mode not offered"
    );
    request["purchased_feature"] = json!(feature);
    if chicken && feature == "buy_bonus" {
        request["feature_id"] = json!(mode.key);
    }
    if !chicken {
        request["isSuperBonus"] = json!(mode.key.ends_with(":super"));
    }
    Ok(request)
}
fn hyper_next(
    result: &Value,
    chicken: bool,
    bet: &Value,
    step: usize,
    stages: usize,
) -> Result<Option<Value>> {
    let final_flag = result["final"]
        .as_bool()
        .ok_or_else(|| unknown_event("BGaming final phase missing"))?;
    if final_flag {
        return Ok(None);
    }
    if chicken {
        if stages == 0 || step >= stages {
            return Err(unknown_event(
                "BGaming Chicken Fire stage sequence exhausted",
            ));
        }
        Ok(Some(json!({"bet":bet,"bet_type":"bet"})))
    } else if step == 0 && result["resp"]["nextAction"] == "BONUS" {
        Ok(Some(
            json!({"bet":bet,"bet_type":"betting","action":"bonus"}),
        ))
    } else {
        Err(unknown_event("BGaming Hyperhive unknown continuation"))
    }
}
fn flat_next(result: &Value, bet: &Value) -> Result<Option<Value>> {
    if result["final"] == true {
        return Ok(None);
    }
    if result["final"] != false
        || ["choices", "gamble", "next_action"]
            .iter()
            .any(|key| !result["resp"][key].is_null())
    {
        return Err(unknown_event("BGaming flat-RPC unknown continuation"));
    }
    Ok(Some(json!({"bet":bet,"bet_type":"bet"})))
}
fn verify_flat_source(source: &str) -> Result<()> {
    require_source(Regex::new(r#"body:\{req:\{bet:[A-Za-z_$][\w$]*,bet_type:[A-Za-z_$][\w$]*\},state_lock:this\.stateLock\},playMethod:"play""#)?.is_match(source)
        && source.contains(".resp.win") && !source.contains("purchased_feature"),"Sweet Samurai flat request")
}

async fn hyper(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    page: Url,
    config: Value,
) -> Result<Completion> {
    ensure!(
        page.host_str()
            .is_some_and(|h| h.ends_with(".demo.bgaming-network.com")),
        "BGaming Hyperhive origin mismatch"
    );
    let token = text(&config, "play_token")?;
    let mut frontend_url = page.join("/")?;
    frontend_url.query_pairs_mut().append_pair("token", token);
    let frontend = get(ctx, "bgaming_frontend", frontend_url.as_str()).await?;
    let endpoint = page.join("/api")?;
    let headers = headers(&page);
    let mut params = json!({"token":token});
    if frontend.contains("src/import-map.json") && frontend.contains("System.import") {
        params["req"] = json!({});
    }
    let id = Uuid::new_v4().to_string();
    let initial = rpc_result(
        ctx.json(
            "bgaming_rpc_init",
            endpoint.as_str(),
            &json!({"id":id,"jsonrpc":"2.0","method":"init","params":params}),
            headers.clone(),
            false,
        )
        .await?,
        &id,
    )?;
    ensure!(
        initial["state"].is_null() && initial["final"] != false,
        "BGaming Hyperhive session has an open round"
    );
    let chicken = game.id == "ChickenFire";
    let flat = game.id == "SweetSamurai";
    let (route, hash) = if flat {
        ("/assets/main/index.js", "")
    } else if chicken {
        (
            "/assets/index-CoAzB5sA.js",
            "3447e6d2577faf09770c691dff91c15e10106981158b523080c99ac887c2ca33",
        )
    } else {
        (
            "/main.js",
            "28652da884003877001e520669c43ca36dabf882c1e05a0df0812035c61c5602",
        )
    };
    let script = Regex::new(&format!(
        r#"<script[^>]+src=["'](?:\./|/)?{}(?:\?[^"']*)?["']"#,
        regex::escape(route.trim_start_matches('/'))
    ))?;
    if !flat && !script.is_match(&frontend) {
        return Err(unknown_event("BGaming Hyperhive native script changed"));
    }
    let source = get(ctx, "bgaming_native_source", page.join(route)?.as_str()).await?;
    if flat {
        verify_flat_source(&source)?;
    } else if sha256(source.as_bytes()) != hash {
        return Err(unknown_event("BGaming Hyperhive client version changed"));
    }
    ctx.save_session(&json!({"provider":"bgaming","game":game.id,"token":token,"endpoint":endpoint.as_str(),"source_sha256":sha256(source.as_bytes())}))?;
    let mut request = if flat {
        json!({"bet":minimum(&initial["config"]["bet_limits"])? ,"bet_type":"bet"})
    } else {
        hyper_entry(&initial, mode, chicken)?
    };
    let bet = request["bet"].clone();
    let mut lock = initial["state_lock"].as_str().unwrap_or("").to_owned();
    let mut round = None;
    let mut stages = 0;
    for step in 0..ctx.max_steps().min(if chicken || flat { 500 } else { 2 }) {
        let id = Uuid::new_v4().to_string();
        let result=rpc_result(ctx.json("bgaming_rpc_play",endpoint.as_str(),&json!({"id":id,"jsonrpc":"2.0","method":"play","params":{"token":token,"state_lock":lock,"req":request}}),headers.clone(),true).await?,&id)?;
        if let Some(id) = native_id(&result["round"]) {
            ensure!(
                round.as_ref().is_none_or(|known| *known == id),
                "BGaming Hyperhive native round changed"
            );
            round.get_or_insert(id);
        }
        if step == 0 && chicken && result["final"] != true {
            stages = result["resp"]["data"]
                .as_array()
                .filter(|a| {
                    !a.is_empty()
                        && a.len() < 500
                        && a.iter().all(|v| v["respinsCount"].as_u64().is_some())
                })
                .ok_or_else(|| unknown_event("BGaming Chicken Fire native stage offer missing"))?
                .len();
        }
        let next = if flat {
            flat_next(&result, &bet)?
        } else {
            hyper_next(&result, chicken, &bet, step, stages)?
        };
        let Some(next) = next else {
            return Ok(Completion {
                native_round_id: round,
                details: json!({"family":"hyperhive","steps":step+1,"mode":mode.key}),
            });
        };
        lock = text(&result, "state_lock")?.into();
        request = next;
    }
    Err(unknown_event(
        "BGaming Hyperhive continuation budget exhausted",
    ))
}
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    ensure!(
        supports(game, mode),
        "BGaming game/mode has no native capture binding"
    );
    let (page, config, html) = launch(ctx, &game.id).await?;
    if let Some(family) = rpc_extra::family(&game.id, mode) {
        return rpc_extra::execute(ctx, game, mode, page, config, family).await;
    }
    if let Some(family) = legacy_extra::family(&game.id, mode) {
        if page.path() == "/hyperhive" {
            return Err(unknown_event("BGaming legacy family changed to RPC"));
        }
        return legacy_extra::execute(ctx, game, mode, page, config, &html, family).await;
    }
    if page.path() == "/hyperhive" {
        if !matches!(
            game.id.as_str(),
            "RedHotChilliChickens" | "ChickenFire" | "SweetSamurai"
        ) {
            return Err(unknown_event(
                "BGaming legacy family changed to unbound Hyperhive client",
            ));
        }
        hyper(ctx, game, mode, page, config).await
    } else {
        legacy(ctx, game, mode, page, config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mode(key: &str, kind: ModeKind) -> Mode {
        Mode {
            key: key.into(),
            kind,
            label: String::new(),
            requires_prerequisites: Value::Null,
        }
    }
    #[test]
    fn generic_legacy_tier_is_registered_without_pretending_rpc_variant_support() {
        let game = Game {
            id: "FormulaX10000".into(),
            name: String::new(),
            modes: vec![],
        };
        assert!(supports(&game, &mode("bonus_buy:3", ModeKind::Purchase)));
        assert!(!supports(&game, &mode("bonus_buy:3", ModeKind::Normal)));
        assert!(!supports(
            &game,
            &mode("unbound_purchase", ModeKind::Purchase)
        ));
        let game = Game {
            id: "MultiRush".into(),
            name: String::new(),
            modes: vec![],
        };
        assert!(supports(&game, &mode("super", ModeKind::Variant)));
        assert!(!supports(&game, &mode("new_variant", ModeKind::Variant)));
    }
    #[test]
    fn tier_request_retains_exact_wire_value_and_unknown_menu_is_typed() {
        let initial = json!({"options":{"available_bets":[20,10], "feature_options":{
            "feature_multipliers":{"bonus_buy":{"0":10000,"3":80000}}}}});
        assert_eq!(
            legacy_entry(&initial, &mode("bonus_buy:3", ModeKind::Purchase)).unwrap(),
            json!({"bet":10,"purchased_feature":"bonus_buy","purchased_feature_level":"3"})
        );
        let err = legacy_entry(&initial, &mode("bonus_buy:4", ModeKind::Purchase)).unwrap_err();
        assert!(err.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn rows_preserve_string_wire_and_native_row_menu_without_money() {
        let initial = json!({"options":{"available_bets":[1],"valid_bets":{"4":[20,10]},"feature_options":{"feature_multipliers":{"freespin_buy":{"4":10000}}}}});
        let source = r#"this.additionalSpinOptions.rows=String(this.reelSize);purchased_feature_level=""+this.reelSize;this.FREESPIN_BUY,this.reelSize"#;
        let plan = legacy_plan(
            &initial,
            &mode("freespin_buy:4@rows:4", ModeKind::Purchase),
            source,
            "BigTunaBonanza",
        )
        .unwrap();
        assert_eq!(
            plan.entry,
            json!({"bet":10,"purchased_feature":"freespin_buy","purchased_feature_level":"4","rows":"4"})
        );
        assert_eq!(plan.continuation_options["freespin"], json!({"rows":"4"}));
        assert!(
            legacy_plan(
                &initial,
                &mode("freespin_buy:4@rows:5", ModeKind::Purchase),
                source,
                "BigTunaBonanza"
            )
            .is_err()
        );
    }
    #[test]
    fn indexed_rows_take_source_defined_index_not_global_minimum() {
        let initial = json!({"options":{"available_bets":[5,20,50]}});
        let source = "var LO=3,HI=5,T={3:1,4:2,5:3};this.reelSize=HI;this.additionalSpinOptions.rows=this.reelSize;initialAvailableBets[this.reelSize-LO];changeReelsSize(x){var m=T[this.reelSize]}";
        let plan = legacy_plan(
            &initial,
            &mode("normal@rows:4", ModeKind::Normal),
            source,
            "BigAtlantisFrenzy",
        )
        .unwrap();
        assert_eq!(plan.entry, json!({"bet":20,"rows":4}));
        assert_eq!(plan.continuation_options["respin"], json!({"rows":4}));
        assert!(
            legacy_plan(
                &initial,
                &mode("normal@rows:4", ModeKind::Normal),
                &source.replace("LO=3", "LO=8"),
                "BigAtlantisFrenzy"
            )
            .is_err()
        );
    }
    #[test]
    fn volatility_paid_only_and_line_mode_persists_through_free_round() {
        let initial = json!({"options":{"available_bets":[10]}});
        let source = r#"this.additionalSpinOptions.volatility=1==this.getCurrentVolatility()?"low":"medium";beforeGoFreespins(t){this.fsModeSwitcher(Number(i)),delete this.additionalSpinOptions.volatility}"#;
        let plan = legacy_plan(
            &initial,
            &mode("normal@base_volatility:low", ModeKind::Normal),
            source,
            "DiscoParty",
        )
        .unwrap();
        assert_eq!(plan.entry, json!({"bet":10,"volatility":"low"}));
        assert!(plan.continuation_options.is_empty());
        let source = r#"var choices=["20","40"],F={20:1,40:2};onLinesCountChanged(e){this.additionalSpinOptions.mode=this.linesCount;this.setBetsVisibleMultiplier(F[e],!0)}"#;
        let plan = legacy_plan(
            &initial,
            &mode("lines:40", ModeKind::Normal),
            source,
            "WildTiger",
        )
        .unwrap();
        assert_eq!(plan.entry, json!({"bet":10,"mode":"40"}));
        assert_eq!(plan.continuation_options["freespin"], json!({"mode":"40"}));
        assert!(
            legacy_plan(
                &initial,
                &mode("lines:60", ModeKind::Normal),
                source,
                "WildTiger"
            )
            .is_err()
        );
    }
    #[test]
    fn gold_selector_removed_for_free_continuation_and_tier_confirmed() {
        let initial = json!({"options":{"available_bets":[10],"feature_options":{"feature_multipliers":{"freespin_buy":{"3":10000}}}}});
        let source = r#"this.BET_BY_SPECIAL_LVL=F;F={1:1,2:2,3:3,4:4,5:5};this.additionalSpinOptions.gold_symbols_count=""+v;this.BET_BY_SPECIAL_LVL[this.goldSymbolsLevel];delete this.additionalSpinOptions.gold_symbols_count"#;
        let plan = legacy_plan(
            &initial,
            &mode("gold_symbols:3/freespin_buy", ModeKind::Purchase),
            source,
            "Maneki88Gold",
        )
        .unwrap();
        assert_eq!(
            plan.entry,
            json!({"bet":10,"gold_symbols_count":"3","purchased_feature":"freespin_buy","purchased_feature_level":"3"})
        );
        assert!(plan.continuation_options.is_empty());
        confirm_purchase(
            &json!({"flow":{"purchased_feature":{"name":"freespin_buy","level":3}}}),
            &plan.entry,
        )
        .unwrap();
        let e = confirm_purchase(
            &json!({"flow":{"purchased_feature":{"name":"freespin_buy","level":4}}}),
            &plan.entry,
        )
        .unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn free_type_is_only_on_free_request_and_portal_is_explicit() {
        let initial = json!({"options":{"available_bets":[10],"feature_options":{"feature_multipliers":{"freespin_buy":10000}}}});
        let source = r#"var X="giant_symbol";this.additionalSpinOptions.freespins_type=X;delete this.additionalSpinOptions.freespins_type"#;
        let plan = legacy_plan(
            &initial,
            &mode("freespin_buy/type:giant_symbol", ModeKind::Purchase),
            source,
            "AlohaKingElvis",
        )
        .unwrap();
        assert!(plan.entry.get("freespins_type").is_none());
        assert_eq!(
            plan.continuation_options["freespin"],
            json!({"freespins_type":"giant_symbol"})
        );
        let e = planned_legacy_next(
            &json!({"flow":{"state":"choice","available_actions":["init","mystery"]}}),
            &plan,
            source,
            "AlohaKingElvis",
        )
        .unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn every_embedded_mode_has_exact_kind_and_unknown_extensions_stay_unsupported() {
        let b: Value =
            serde_json::from_str(include_str!("../../bindings/bgaming-legacy-v2.json")).unwrap();
        let mut total = 0;
        for (id, g) in b["games"].as_object().unwrap() {
            let game = Game {
                id: id.clone(),
                name: String::new(),
                modes: vec![],
            };
            for (key, kind) in g["modes"].as_object().unwrap() {
                let m = mode(key, serde_json::from_value(kind.clone()).unwrap());
                assert!(supports(&game, &m));
                total += 1;
            }
            assert!(!supports(
                &game,
                &mode("unreviewed/feature", ModeKind::Special)
            ));
        }
        assert!(total > 500);
    }
    #[test]
    fn sweet_samurai_flat_rpc_is_distinct_from_bonus_action_protocol() {
        let game = Game {
            id: "SweetSamurai".into(),
            name: String::new(),
            modes: vec![],
        };
        assert!(supports(&game, &mode("normal", ModeKind::Normal)));
        assert!(!supports(&game, &mode("buy_bonus", ModeKind::Purchase)));
    }
    #[test]
    fn flat_rpc_final_needs_no_win_balance_and_continuation_strips_purchase() {
        assert!(
            flat_next(&json!({"final":true}), &json!(10))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            flat_next(&json!({"final":false,"resp":{}}), &json!(10))
                .unwrap()
                .unwrap(),
            json!({"bet":10,"bet_type":"bet"})
        );
        let err = flat_next(
            &json!({"final":false,"resp":{"next_action":"novel"}}),
            &json!(10),
        )
        .unwrap_err();
        assert!(err.downcast_ref::<crate::model::UnknownEvent>().is_some());
        verify_flat_source(r#"body:{req:{bet:a,bet_type:b},state_lock:this.stateLock},playMethod:"play";model.resp.win"#).unwrap();
        assert!(verify_flat_source("unrecognized").is_err());
    }
    #[test]
    fn frenzy_natural_bonus_uses_source_default_portal_and_explicit_modes_override_it() {
        let initial = json!({"options":{"available_bets":[10],"feature_options":{"feature_multipliers":{"freespin_chance":120,"freespin_buy":{"0":5000,"1":10000,"2":20000}}}}});
        let source = "selected_portal";
        for key in [
            "normal",
            "freespin_chance",
            "freespin_buy:0",
            "freespin_buy:1",
            "freespin_buy:2/portal:4",
        ] {
            let plan = legacy_plan(
                &initial,
                &mode(key, ModeKind::Purchase),
                source,
                "FrenzyClusters",
            )
            .unwrap();
            let expected = if key.ends_with("/portal:4") { 4 } else { 0 };
            assert_eq!(
                plan.continuation_options["gamble_bonus"],
                json!({"selected_portal":expected})
            );
            assert_eq!(planned_legacy_next(&json!({"flow":{"state":"gamble_bonus","available_actions":["init","gamble_bonus"]}}),&plan,source,"FrenzyClusters").unwrap(),Some("gamble_bonus"));
        }
    }
    #[test]
    fn unrelated_free_type_literal_is_rejected_and_selected_type_requires_confirmation() {
        let initial = json!({"options":{"available_bets":[10],"feature_options":{"feature_multipliers":{"freespin_buy":10000}}}});
        let source = r#"var X="unknown",Y="giant_symbol";this.additionalSpinOptions.freespins_type=X;delete this.additionalSpinOptions.freespins_type"#;
        assert!(
            legacy_plan(
                &initial,
                &mode("freespin_buy/type:giant_symbol", ModeKind::Purchase),
                source,
                "AlohaKingElvis"
            )
            .is_err()
        );
        let error = confirm_purchase(
            &json!({"features":{"freespins_type":"other"}}),
            &json!({"freespins_type":"giant_symbol"}),
        )
        .unwrap_err();
        assert!(error.downcast_ref::<crate::model::UnknownEvent>().is_some());
        assert!(confirm_purchase(&json!({}), &json!({"freespins_type":"giant_symbol"})).is_err());
        confirm_purchase(
            &json!({"features":{"freespins_type":"giant_symbol"}}),
            &json!({"freespins_type":"giant_symbol"}),
        )
        .unwrap();
    }
    #[test]
    fn indexed_and_gold_selector_domains_must_come_from_native_source_tables() {
        let initial = json!({"options":{"available_bets":[5,20,50]}});
        let source = "var LO=3;this.additionalSpinOptions.rows=this.reelSize;initialAvailableBets[this.reelSize-LO]";
        assert!(
            legacy_plan(
                &initial,
                &mode("normal@rows:4", ModeKind::Normal),
                source,
                "BigAtlantisFrenzy"
            )
            .is_err()
        );
        let source = r#"this.additionalSpinOptions.gold_symbols_count=""+v;this.BET_BY_SPECIAL_LVL[this.goldSymbolsLevel];delete this.additionalSpinOptions.gold_symbols_count"#;
        assert!(
            legacy_plan(
                &initial,
                &mode("gold_symbols:3/normal", ModeKind::Normal),
                source,
                "Maneki88Gold"
            )
            .is_err()
        );
    }
    #[test]
    fn legacy_closed_without_wallet_or_win_and_unknown_is_typed() {
        assert!(
            legacy_next(&json!({"flow":{"state":"closed"}}))
                .unwrap()
                .is_none()
        );
        let e = legacy_next(
            &json!({"flow":{"state":"pick","available_actions":["init","new_choice"]}}),
        )
        .unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn hyper_native_final_without_balance_or_win_and_unknown_is_typed() {
        assert!(
            hyper_next(&json!({"final":true}), false, &json!(10), 0, 0)
                .unwrap()
                .is_none()
        );
        let e = hyper_next(
            &json!({"final":false,"resp":{"nextAction":"NOVEL"}}),
            false,
            &json!(10),
            0,
            0,
        )
        .unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn native_free_continuation_never_repeats_purchase() {
        let request = hyper_next(
            &json!({"final":false,"resp":{"nextAction":"BONUS"}}),
            false,
            &json!(10),
            0,
            0,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            request,
            json!({"bet":10,"bet_type":"betting","action":"bonus"})
        );
        assert!(request.get("purchased_feature").is_none());
    }
    #[test]
    fn legacy_default_request_needs_no_financial_metadata() {
        let mode = Mode {
            key: "normal".into(),
            kind: ModeKind::Normal,
            label: String::new(),
            requires_prerequisites: Value::Null,
        };
        assert_eq!(
            legacy_entry(&json!({"options":{"available_bets":[20,10]}}), &mode).unwrap(),
            json!({"bet":10})
        );
    }
    #[test]
    fn rpc_envelope_checks_correlation_without_money() {
        let result = rpc_result(
            json!({"id":"r1","jsonrpc":"2.0","result":{"final":true}}),
            "r1",
        )
        .unwrap();
        assert_eq!(result["final"], true);
        assert!(
            rpc_result(json!({"id":"other","jsonrpc":"2.0","result":{}}), "r1")
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
