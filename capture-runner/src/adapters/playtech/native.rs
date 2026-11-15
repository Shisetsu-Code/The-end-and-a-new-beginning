//! Rust translation of supplied primitives.py / command_bindings.py contracts.
//! No financial ledger: only native state controls completion.
use super::*;
use base64::Engine;
use flate2::{Decompress, FlushDecompress, Status};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const CLASS: &str = "com.pt.gpas.platform.game.OgwCorosinGameCommand";
pub(super) const END: &str = "BET_END_STATE_IDENTIFIER";
const MAX_PACKED: usize = 8 * 1024 * 1024;
#[derive(Debug)]
pub(super) struct Contract {
    pub legacy: bool,
    pub omit_empty_entry: bool,
    pub pick_bound: bool,
    pub binding: Value,
}
#[derive(Debug)]
pub(super) struct Offer {
    pub id: String,
    pub selector: String,
    pub bet_type: String,
    pub cost: u64,
    pub multiplier: u64,
    pub parameters: Value,
    pub lines: Option<Value>,
}
fn matches(pattern: &str, text: &str) -> bool {
    Regex::new(pattern).expect("static regex").is_match(text)
}

pub(super) fn source_contract(
    manifest: &Value,
    sources: &[String],
    binding: &Value,
) -> Result<Contract> {
    ensure!(
        manifest["env"]["glu6"] != true,
        "GPAS GLU6 bridge assets are not implemented"
    );
    ensure!(
        binding["requires_complete_native_request_producer"] != true,
        "GPAS complete native producer unavailable"
    );
    let text = sources.join("\n");
    ensure!(
        text.contains(CLASS) && matches(r"\b40021\b", &text) && matches(r"\b48775\b", &text),
        "GPAS client command constructor not bound"
    );
    if let Some(pins) = binding["source_sha256"].as_array() {
        let expected: BTreeSet<_> = pins
            .iter()
            .map(|v| v.as_str().context("GPAS invalid source pin"))
            .collect::<Result<_>>()?;
        let hashes: BTreeSet<String> = sources
            .iter()
            .map(|s| {
                Sha256::digest(s.as_bytes())
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect()
            })
            .collect();
        ensure!(
            expected == hashes.iter().map(String::as_str).collect(),
            "GPAS official command source changed"
        );
    }
    let legacy = manifest["setup"]["usesLegacyBetApi"] == true;
    if legacy {
        ensure!(
            text.contains("GLU.game.bet") && text.contains("stakeCostUuid"),
            "GPAS legacy constructor not bound"
        );
    } else {
        ensure!(
            text.contains("GLU.game.playMoney")
                && (manifest["setup"]["usesLegacyBetApi"] == false
                    || manifest["setup"]["useExtendedBetApi"] == true
                    || !text.contains("GLU.game.bet")),
            "GPAS modern constructor ambiguous"
        );
    }
    Ok(Contract {
        legacy,
        omit_empty_entry: matches(
            r"var [\w$]+=\{ID:[\w$]+\};.{0,120}[\w$]+&&\([\w$]+\.parameters=JSON\.parse\(",
            &text,
        ),
        pick_bound: binding["pick_bound"] == true
            || (text.contains("FGPickController") && matches(r#"["']pick["']"#, &text)),
        binding: binding.clone(),
    })
}

pub(super) fn decode_data(data: &Value) -> Result<Value> {
    ensure!(data.is_object(), "GPAS gameData is not an object");
    if data["_type"] != "gpas:packed:zlib" {
        return Ok(data.clone());
    }
    let payload = field(data, "payload")?;
    ensure!(payload.len() <= MAX_PACKED * 2, "GPAS packed input limit");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .context("GPAS invalid packed base64")?;
    let mut decoder = Decompress::new(true);
    let mut output = vec![0; MAX_PACKED + 1];
    let status = decoder
        .decompress(&bytes, &mut output, FlushDecompress::Finish)
        .context("GPAS invalid packed zlib")?;
    ensure!(
        status == Status::StreamEnd
            && decoder.total_in() as usize == bytes.len()
            && decoder.total_out() as usize <= MAX_PACKED,
        "GPAS packed stream truncated, trailing or too large"
    );
    output.truncate(decoder.total_out() as usize);
    let data: Value = serde_json::from_slice(&output)?;
    ensure!(data.is_object(), "GPAS packed body is not an object");
    Ok(data)
}

pub(super) fn entry(o: &Offer, c: &Contract) -> Result<Value> {
    let total = o
        .cost
        .checked_mul(o.multiplier)
        .context("GPAS cost overflow")?;
    ensure!(o.parameters.is_array(), "GPAS entry parameters not bound");
    let mut data = json!({"_type":CLASS,"ID":40021,"parameters":o.parameters});
    if c.omit_empty_entry && o.parameters.as_array().is_some_and(Vec::is_empty) {
        data.as_object_mut().unwrap().remove("parameters");
    }
    if c.legacy {
        if let Some(lines) = &o.lines {
            ensure!(
                lines
                    .as_array()
                    .is_some_and(|a| !a.is_empty() && a.iter().all(|v| v == 0 || v == 1)),
                "GPAS invalid native line vector"
            );
            data = json!({"_type":CLASS,"ID":40021,"bet":total,"lines":lines});
        } else {
            data["bet"] = json!(total);
            data["stakeCostUuid"] = json!(o.id);
            data["baseCostMultiplier"] = json!(o.multiplier);
        }
        Ok(json!({"_type":COMMAND,"gameData":data,"betType":"default"}))
    } else {
        Ok(
            json!({"_type":COMMAND,"gameData":data,"autoplay":false,"betCost":total,"betType":o.bet_type,"featureType":o.selector}),
        )
    }
}

pub(super) fn continuation(c: &Contract, mut parameters: Value) -> Result<Value> {
    let p = parameters
        .as_array_mut()
        .context("GPAS continuation parameters not array")?;
    if c.binding["numeric_pick_parameters"] == true {
        for item in p.iter_mut().filter(|p| p["key"] == "pick") {
            if let Some(s) = item["value"].as_str() {
                item["value"] = json!(s.parse::<u64>().context("GPAS pick must be numeric")?);
            }
        }
    }
    if let Some(suffix) = c.binding["continuation_parameter_suffix"].as_array() {
        p.extend(suffix.clone());
    }
    let mut keys = BTreeSet::new();
    ensure!(
        p.iter()
            .all(|v| v["key"].as_str().is_some_and(|k| keys.insert(k))),
        "GPAS continuation parameter collision"
    );
    let mut command =
        json!({"_type":COMMAND,"gameData":{"_type":CLASS,"ID":48775,"parameters":parameters}});
    if !c.legacy {
        command["autoplay"] = json!(false)
    }
    Ok(command)
}

fn inactive(config: &Value, row: &Value, index: usize, c: &Contract) -> Result<()> {
    let game = &config["game"];
    let id = field(row, "uuid")?;
    let name = row["name"].as_str().unwrap_or("");
    if let Some(active) = game.get("activeBetOptions") {
        let a = active.as_array().context("GPAS invalid activeBetOptions")?;
        ensure!(
            a.iter().all(Value::is_string)
                && (a.iter().any(|v| v == name)
                    || (index == 0 && a.iter().any(|v| v == "default"))),
            "GPAS requested option inactive"
        );
    }
    let lower = name.to_ascii_lowercase();
    ensure!(
        !(game["featureBetEnabled"] == false
            && matches(
                r"^(?:bf[0-9]*|buy[ _-]?(?:a[ _-]?)?feature.*|buy[ _-]?bonus.*)$",
                &lower
            )),
        "GPAS featureBetEnabled=false"
    );
    ensure!(
        !(game["extraBetEnabled"] == false
            && matches(
                r"^(?:extra(?:bet)?(?:[0-9].*)?|ante(?:bet)?(?:[0-9].*)?)$",
                &lower
            )),
        "GPAS extraBetEnabled=false"
    );
    for key in ["inactive_token_mode_ids", "inactive_powerplay_mode_ids"] {
        ensure!(
            !c.binding[key]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == id)),
            "GPAS source-declared inactive mode"
        );
    }
    ensure!(
        c.binding["inactive_internal_mode_reasons"]
            .get(id)
            .is_none(),
        "GPAS internal profile is inactive"
    );
    if let Some(required) = c.binding["inactive_unlockable_modes"][id].as_str() {
        ensure!(
            config["modifiers"]["active"]["unlockables"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v["name"] == required)),
            "GPAS required unlockable absent"
        );
    }
    if let Some(exclusive) = c.binding.get("exclusive_extra_profile") {
        ensure!(
            game["bothBetOptionsEnabled"] == false && game["minBet"] == exclusive["cost"],
            "GPAS exclusive option profile changed"
        );
        ensure!(
            exclusive["extra_id"] == id,
            "GPAS base profile inactive in exclusive extra variant"
        );
    }
    if let Some(disabled) = c.binding.get("inactive_source_disabled_extra") {
        ensure!(
            disabled["extra_id"] != id,
            "GPAS extra disabled by source variant"
        );
        ensure!(
            game["extraBetEnabled"].is_null() || game["extraBetEnabled"] == false,
            "GPAS extra availability changed"
        );
        ensure!(
            game["bothBetOptionsEnabled"] == false
                && game["activeBetOptions"].is_null()
                && config["limits"]["minBet"] == disabled["minimum"],
            "GPAS source availability profile changed"
        );
        let rows = game["costConfiguration"]
            .as_array()
            .context("GPAS costs missing")?;
        let actual: serde_json::Map<String, Value> = rows
            .iter()
            .map(|r| Ok((field(r, "uuid")?.into(), r.clone())))
            .collect::<Result<_>>()?;
        ensure!(
            Value::Object(actual) == disabled["costs_by_id"],
            "GPAS source cost domain changed"
        );
    }
    Ok(())
}

/// Exact requested UUID is mandatory. Display names never substitute for identity.
pub(super) fn resolve(
    config: &Value,
    mode: &Mode,
    c: &Contract,
    text: &str,
    maps: &Value,
    settings: &Value,
) -> Result<Offer> {
    resolve_inner(config, mode, c, text, maps, settings, false)
}

pub(super) fn resolve_scoped(
    rows: &[Value],
    mode: &Mode,
    c: &Contract,
    text: &str,
    maps: &Value,
    settings: &Value,
) -> Result<Offer> {
    let base = configuration(rows, &json!({"betType":"default"}))?;
    // Discover the native selector before consulting the feature's stake bounds.
    // The candidate is never dispatched and carries no selected stake context.
    let candidate = resolve_inner(&base, mode, c, text, maps, &Value::Null, true)?;
    let scoped = configuration(
        rows,
        &json!({"betType":candidate.bet_type,"featureType":candidate.selector}),
    )?;
    let offer = resolve(&scoped, mode, c, text, maps, settings)?;
    ensure!(
        offer.selector == candidate.selector && offer.bet_type == candidate.bet_type,
        "GPAS scoped selector changed"
    );
    Ok(offer)
}

fn resolve_inner(
    config: &Value,
    mode: &Mode,
    c: &Contract,
    text: &str,
    maps: &Value,
    settings: &Value,
    selector_only: bool,
) -> Result<Offer> {
    let game = &config["game"];
    let costs = game["costConfiguration"]
        .as_array()
        .context("GPAS cost menu missing")?;
    let mut ids = BTreeSet::new();
    ensure!(
        costs
            .iter()
            .all(|r| r["uuid"].as_str().is_some_and(|s| ids.insert(s))),
        "GPAS duplicate or missing cost UUID"
    );
    let (index, row) = costs
        .iter()
        .enumerate()
        .find(|(_, r)| r["uuid"] == mode.key)
        .context("GPAS reviewed UUID not in fresh configuration")?;
    let observed = c.binding.get("observed_default_baseline");
    if let Some(profile) = observed {
        let lines: BTreeSet<_> = costs
            .iter()
            .map(|r| r["parameters"]["winline"].as_str())
            .collect();
        ensure!(
            profile["id"] == mode.key
                && row["cost"] == profile["cost"]
                && config["limits"]["minBet"] == profile["cost"]
                && game["extraBetEnabled"] == false
                && lines.len() == 1
                && !lines.contains(&None),
            "GPAS observed baseline profile changed"
        );
    } else {
        inactive(config, row, index, c)?;
    }
    let cost = row["cost"]
        .as_u64()
        .filter(|v| *v > 0)
        .context("GPAS invalid cost")?;
    let multiplier = if selector_only {
        1
    } else {
        let factors = game
            .get("baseCostMultipliers")
            .or_else(|| game.get("coinSizes"))
            .and_then(Value::as_array)
            .context("GPAS multiplier domain missing")?;
        let factors: Vec<u64> = factors
            .iter()
            .map(|v| {
                v.as_u64()
                    .filter(|v| *v > 0)
                    .context("GPAS unsupported noninteger or invalid multiplier")
            })
            .collect::<Result<_>>()?;
        let bound = |key: &str| -> Result<Option<u64>> {
            config["limits"]
                .get(key)
                .or_else(|| game.get(key))
                .map(|v| {
                    v.as_u64()
                        .with_context(|| format!("GPAS invalid native {key}"))
                })
                .transpose()
        };
        let minimum = bound("minBet")?;
        let maximum = bound("maxBet")?;
        ensure!(
            minimum.zip(maximum).is_none_or(|(min, max)| min <= max),
            "GPAS inverted native stake bounds"
        );
        let legal: Vec<u64> = factors
            .into_iter()
            .filter(|m| {
                cost.checked_mul(*m).is_some_and(|amount| {
                    minimum.is_none_or(|min| amount >= min)
                        && maximum.is_none_or(|max| amount <= max)
                })
            })
            .collect();
        let multiplier = if let Some(value) = settings.get("multiplier") {
            value
                .as_u64()
                .context("GPAS invalid requested multiplier")?
        } else {
            *legal
                .iter()
                .min()
                .context("GPAS empty legal multiplier domain")?
        };
        ensure!(
            legal.contains(&multiplier),
            "GPAS requested multiplier outside native stake domain"
        );
        if let Some(required) = c.binding.get("required_parameter_multiplier") {
            ensure!(
                required == multiplier,
                "GPAS native parameter multiplier changed"
            );
        }
        if c.binding["minimum_native_context_only"] == true {
            ensure!(
                multiplier == 1,
                "GPAS producer bound to native context multiplier 1 only"
            );
        }
        multiplier
    };
    let name = row["name"].as_str().unwrap_or("");
    let baseline_override = json!({"selector":"default","parameters":[]});
    let pinned = if observed.is_some() {
        &baseline_override
    } else {
        &c.binding["modes"][&mode.key]
    };
    if observed.is_some() {
        ensure!(multiplier == 1, "GPAS observed baseline multiplier unbound");
    }
    let scope = &game["namedScopes"][name];
    let aliases = super::sources::selectors(text, costs)?;
    let selector = pinned["selector"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| aliases.get(name).cloned())
        .or_else(|| scope["featureType"].as_str().map(str::to_owned))
        .or_else(|| {
            ((name == "default" || name == "base") && text.contains("baseCostName:"))
                .then(|| name.to_string())
        })
        .context("GPAS native feature selector unbound")?;
    // A pinned native constructor supplies selector/parameters directly. Generic receipt modes
    // must also bind their category to fresh client declarations/scopes.
    if pinned.is_null() {
        let mapped = super::sources::mapped_kind(name, maps);
        let declared = super::sources::declared_kind(name, text);
        let scoped = scope["betType"] == "default" && scope["featureType"] == name;
        let lower = name.to_ascii_lowercase();
        let kind = declared.or(mapped).or_else(|| {
            if (lower == "extra"
                || (scoped && matches(r"^extra(?:[0-9]+|x[0-9]+(?:low|mid|high)?)$", &lower)))
                && matches(
                    r"ExtraBets?(?:Module|Controller|Configuration|Widget)|extraBetNum\s*:\s*1",
                    text,
                )
            {
                Some(ModeKind::AnteBet)
            } else if (lower == "bf"
                || lower == "buyfeature"
                || ((scoped || game["featureBetEnabled"] == true)
                    && matches(r"^(?:bf[0-9]+|buyfeature(?:[0-9]+x|extra))$", &lower)))
                && matches(r"BuyFeature(?:Module|Controller|Configuration)", text)
            {
                Some(ModeKind::Purchase)
            } else if lower == "default"
                || lower == "base"
                || (scoped
                    && (text.contains("currentVolatilitySuffix")
                        || text.contains("VolatilityController"))
                    && matches(r"^default(?:low|mid|high)$", &lower))
                || (aliases.contains_key(name) && row["parameters"]["startingMode"] == "Standard")
            {
                Some(ModeKind::Normal)
            } else {
                None
            }
        });
        ensure!(
            kind == Some(mode.kind),
            "GPAS requested mode category not source-bound"
        );
    }
    let parameters = pinned.get("parameters").cloned().unwrap_or(json!([]));
    ensure!(
        parameters.is_array(),
        "GPAS special request producer unsupported"
    );
    Ok(Offer {
        id: mode.key.clone(),
        selector,
        bet_type: scope["betType"].as_str().unwrap_or("default").into(),
        cost,
        multiplier,
        parameters,
        lines: pinned.get("legacy_line_array").cloned(),
    })
}

#[derive(Default)]
pub(super) struct Picks {
    consumed: BTreeMap<String, BTreeSet<u64>>,
    selected: BTreeMap<String, BTreeSet<u64>>,
}
#[derive(Debug)]
pub(super) enum Step {
    Closed,
    Continue(Value),
}
fn unknown(message: impl Into<String>) -> anyhow::Error {
    crate::model::unknown_event(message)
}

pub(super) fn next_step(
    response: &Value,
    c: &Contract,
    picks: &mut Picks,
    targets: &Value,
) -> Result<Step> {
    let data = decode_data(&response["gameData"])?;
    let phase = data["nextRound"]
        .as_str()
        .ok_or_else(|| unknown("GPAS nextRound missing"))?;
    let stacks = data["playStack"].as_array();
    let alternate = c.binding["terminal_rounds"]
        .as_array()
        .is_some_and(|a| a.iter().any(|v| v == phase));
    if response["gameRoundClosed"] == true && (phase == END || alternate) {
        if alternate {
            ensure!(
                stacks.is_some_and(|a| !a.is_empty()
                    && a.iter().any(|s| s["isLastPlayMode"] == true)
                    && a.iter().all(|s| s["remainingPlayCount"].as_i64() == Some(0)
                        && s["isNextPlayMode"] != true)),
                "GPAS terminal has pending plays"
            );
        }
        return Ok(Step::Closed);
    }
    if response["gameRoundClosed"] != false || phase == END {
        return Err(unknown("GPAS inconsistent native round closure"));
    }
    let stacks = stacks.ok_or_else(|| {
        unknown(format!(
            "GPAS unknown continuation {phase}: playStack missing"
        ))
    })?;
    if stacks.iter().any(|s| !s.is_object()) {
        return Err(unknown("GPAS truncated or invalid playStack"));
    }
    for s in stacks {
        if let (Some(round), Some(results)) = (
            s["round"].as_str(),
            s["lastPlayInModeData"]["listPickData"]["payoutResults"].as_array(),
        ) {
            for result in results {
                if let Some(i) = result["context"]["uiIndex"].as_u64() {
                    picks.consumed.entry(round.into()).or_default().insert(i);
                }
            }
        }
    }
    let active: Vec<_> = stacks
        .iter()
        .filter(|s| s["round"] == phase && s["remainingPlayCount"].as_u64().is_some_and(|n| n > 0))
        .collect();
    if active.len() != 1 {
        return Err(unknown(format!(
            "GPAS unknown or ambiguous continuation {phase}"
        )));
    }
    let kind = active[0]["modeType"].as_str().unwrap_or("");
    let parameters = if kind == "LIST_PICK" {
        if c.binding.get("natural_bonus_gate_adapter").is_some() {
            return Err(unknown("GPAS natural bonus choice gate is not implemented"));
        }
        if let Some(fixed) = c.binding["fixed_continuation_parameters"].get(phase) {
            ensure!(
                data["listPickPlayerPickOptions"]
                    .as_array()
                    .is_none_or(Vec::is_empty),
                "GPAS fixed pick collides with offered options"
            );
            fixed.clone()
        } else {
            if !c.pick_bound {
                return Err(unknown("GPAS pick constructor not source-bound"));
            }
            let offered: Vec<u64> = if let Some(options) = data["listPickPlayerPickOptions"]
                .as_array()
                .filter(|a| !a.is_empty())
            {
                options
                    .iter()
                    .map(|o| o["index"].as_u64().context("GPAS invalid pick option"))
                    .collect::<Result<_>>()?
            } else if let Some(domain) = c.binding["pick_domains"][phase].as_array() {
                domain
                    .iter()
                    .map(|i| i.as_u64().context("GPAS invalid source pick domain"))
                    .collect::<Result<_>>()?
            } else {
                return Err(unknown("GPAS pick domain unknown"));
            };
            let offered: BTreeSet<_> = offered.into_iter().collect();
            let consumed = picks.consumed.entry(phase.into()).or_default();
            let selected = picks.selected.entry(phase.into()).or_default();
            let available: BTreeSet<_> = offered
                .iter()
                .copied()
                .filter(|i| !consumed.contains(i))
                .collect();
            let choice = if let Some(target) = targets.get(phase).filter(|_| selected.is_empty()) {
                let i = target.as_u64().context("GPAS choice must be integer")?;
                ensure!(available.contains(&i), "GPAS requested choice not offered");
                i
            } else {
                *available
                    .first()
                    .ok_or_else(|| unknown("GPAS no unconsumed pick"))?
            };
            selected.insert(choice);
            json!([{"key":"pick","value":choice.to_string()}])
        }
    } else if matches!(kind, "SLOTS") {
        ensure!(
            data["listPickPlayerPickOptions"]
                .as_array()
                .is_none_or(Vec::is_empty),
            "GPAS unexpected offered choices in automatic phase"
        );
        json!([])
    } else {
        return Err(unknown(format!(
            "GPAS unknown continuation modeType {kind}"
        )));
    };
    Ok(Step::Continue(continuation(c, parameters)?))
}

/// Supplier configuration cache semantics: all scope keys must match; object
/// fields merge in wire order, while arrays and scalar fields are replaced.
pub(super) fn configuration(rows: &[Value], context: &Value) -> Result<Value> {
    fn merge(left: &mut Value, right: &Value) {
        if let (Some(a), Some(b)) = (left.as_object_mut(), right.as_object()) {
            for (key, value) in b {
                if let Some(previous) = a.get_mut(key) {
                    merge(previous, value)
                } else {
                    a.insert(key.clone(), value.clone());
                }
            }
        } else {
            *left = right.clone();
        }
    }
    ensure!(
        rows.len() <= 1024 && context.as_object().is_some_and(|o| !o.is_empty()),
        "GPAS invalid configuration domain"
    );
    let mut result = json!({});
    let mut found = false;
    for row in rows {
        let scope = row["scope"]
            .as_object()
            .context("GPAS malformed configuration scope")?;
        ensure!(
            row["content"].is_object(),
            "GPAS malformed configuration content"
        );
        if scope.iter().all(|(key, value)| {
            context.get(key).is_some_and(|actual| {
                if let Some(a) = value.as_array() {
                    a.contains(actual)
                } else {
                    value == actual
                }
            })
        }) {
            merge(&mut result, &row["content"]);
            found = true;
        }
    }
    ensure!(found, "GPAS no matching native configuration scope");
    Ok(result)
}
