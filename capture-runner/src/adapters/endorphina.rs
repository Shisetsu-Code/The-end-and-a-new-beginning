//! Native capture connector derived from standalone info/endorphina/codigo/endorphina.rs.
//! Published source SHA-256: 860b01287f7d03b60d065368d30852c95c85af436b3c613e6a7cd10d450a8df0
//! Reference transport: game-runner/src/adapters/endorphina.rs; no runtime dependency.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal},
};
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::Url;

fn amount(v: &Value) -> Result<Decimal> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => bail!("missing native amount"),
    };
    let n = decimal(&s)?;
    ensure!(n >= Decimal::ZERO, "negative native amount");
    Ok(n)
}
fn parameters(table: &Value, history: &Value) -> Result<Value> {
    ensure!(
        history["status"] == "FINISHED",
        "fresh session has unfinished round"
    );
    let mut p = history["betting_param"].clone();
    let map = p.as_object_mut().context("missing betting parameters")?;
    for (field, set) in [
        ("bet_per_line", "bets_set"),
        ("num_of_lines", "lines_set"),
        ("denom", "denoms_set"),
    ] {
        let target = amount(map.get(field).context("missing bet parameter")?)?;
        let offered = table[set].as_array().context("missing bet offer")?;
        ensure!(
            offered.iter().any(|v| amount(v).is_ok_and(|n| n == target)),
            "unpublished initial bet: {field}"
        );
    }
    map.insert("extra_bet".into(), json!(0));
    map.insert("bonusId".into(), json!(""));
    map.remove("buyInId");
    Ok(p)
}
fn next_step(history: &Value, config: &Value) -> Result<Option<(&'static str, Value)>> {
    if history["status"] == "FINISHED" {
        return Ok(None);
    }
    ensure!(
        history["status"].as_str().is_some_and(|s| !s.is_empty()),
        "missing game status"
    );
    let features = history["features"].as_array().context("missing features")?;
    if let Some(f) = features.iter().rev().find(|f| f["status"] == "IN_PROGRESS") {
        let state = &f["current_state"];
        let fid = f["feature_id"]
            .as_str()
            .ok_or_else(|| crate::model::unknown_event("missing feature id"))?;
        if state["is_final"] == true
            || state["isFinal"] == true
            || ((fid == "PICKME" || fid == "WHEEL_WIN") && state["tries"] == 0)
        {
            return Ok(Some(("END_FEATURE", Value::Null)));
        }
        let action = match fid {
            "SLOT" => {
                if !state["curr_reaction"].is_null() && state["curr_reaction"] != false {
                    "logic.api.gamelogic.slot.Drop_Action"
                } else {
                    "logic.api.gamelogic.slot.Slot_Action"
                }
            }
            "FREE_GAME" => {
                let selecting = state["played_free_games"] == 0
                    || (state["not_played_free_games"] == 0
                        && state["number_of_lucky_free_games_retriggers"]
                            .as_i64()
                            .unwrap_or(0)
                            > 0);
                let configs = config["game"]["features"]
                    .as_array()
                    .context("missing published feature configuration")?;
                let selection = configs.iter().find(|f| {
                    f["featureID"] == "FREE_GAME"
                        && f["featureClassName"].as_str().is_some_and(|name| {
                            name.contains("SelectionFreegamesFeature")
                                || name == "ChunjieFreegamesFeature"
                        })
                });
                if selecting && let Some(selection) = selection {
                    let state_choices = state["states"]
                        .as_array()
                        .and_then(|a| a.iter().find_map(|s| s["choice"].as_array()))
                        .filter(|a| !a.is_empty());
                    let multipliers = selection["params"]["multipliers"]
                        .as_array()
                        .filter(|a| !a.is_empty());
                    let selected = if state_choices.is_some() || multipliers.is_some() {
                        0
                    } else {
                        let inputs = config["input"]
                            .as_array()
                            .context("missing published free-game inputs")?;
                        inputs
                            .iter()
                            .flat_map(|c| c["actions"]["onClick"].as_array().into_iter().flatten())
                            .filter(|a| a["event"]["className"] == "FREEGAMES_SELECT")
                            .filter_map(|a| a["event"]["arg"].as_u64())
                            .min()
                            .context("missing published free-game selection")?
                    };
                    return Ok(Some((
                        "STEP_FEATURE",
                        json!({"@type":"logic.api.gamelogic.freegames.Free_Games_Choose_Type_Action","player_choice":selected}),
                    )));
                }
                if state["not_played_free_games"] == 0 && state["is_drop"] != true {
                    return Ok(Some(("END_FEATURE", Value::Null)));
                }
                if state["is_drop"] == true {
                    "logic.api.gamelogic.freegames.Free_Games_Drop_Action"
                } else {
                    "logic.api.gamelogic.freegames.Free_Games_Action"
                }
            }
            "RABBITS_WIN" => {
                if state["tries"].as_i64().is_some_and(|v| v <= 0) {
                    return Ok(Some(("END_FEATURE", Value::Null)));
                }
                "logic.api.gamelogic.rabbits.Rabbits_Action"
            }
            "WHEEL_WIN" => "logic.api.gamelogic.wheel.Wheel_Action",
            "COCKTAIL_WIN" => {
                if state["tries"].as_i64().context("missing tries")? <= 0 {
                    return Ok(Some(("END_FEATURE", Value::Null)));
                }
                "logic.api.gamelogic.cocktail_wins.Cocktail_Action"
            }
            "PICKME" | "MATCH3" | "PICKME_EXTENDED" | "CHANCE_WIN" | "PENALTY" | "PYRAMID_WIN" => {
                let (kind, field, choice) = choice_action(fid, state)?;
                let mut payload = json!({"@type":kind});
                payload[field] = json!(choice);
                return Ok(Some(("STEP_FEATURE", payload)));
            }
            _ => {
                return Err(crate::model::unknown_event(format!(
                    "Endorphina unsupported active feature {fid}"
                )));
            }
        };
        return Ok(Some(("STEP_FEATURE", json!({"@type":action}))));
    }
    if let Some(f) = features
        .iter()
        .find(|f| f["status"] == "NOT_STARTED" && f["feature_id"] != "GAMBLE")
    {
        let fid = f["feature_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| crate::model::unknown_event("missing feature id"))?;
        return Ok(Some((
            "START_FEATURE",
            json!(["logic.api.gamelogic.FeatureID", fid]),
        )));
    }
    if !(features
        .iter()
        .all(|f| matches!(f["status"].as_str(), Some("FINISHED" | "NOT_STARTED"))))
    {
        return Err(crate::model::unknown_event(
            "unknown feature status".to_string(),
        ));
    };
    Ok(Some(("END_GAME", Value::Null)))
}
fn choice_action(fid: &str, state: &Value) -> Result<(&'static str, &'static str, usize)> {
    let (kind, field, cells, hidden): (&str, &str, &Value, bool) = match fid {
        "PICKME" => (
            "logic.api.gamelogic.pickme.PickMe_Action",
            "player_choice",
            &state["field"],
            true,
        ),
        "MATCH3" => (
            "logic.api.gamelogic.match3.Match3_Action",
            "player_choice",
            &state["field"],
            true,
        ),
        "PICKME_EXTENDED" => {
            let level = state["level"].as_u64().context("missing level")? as usize;
            (
                "logic.api.gamelogic.pickmeExtended.PickMeExtendedAction",
                "player_choice",
                &state["items"][level],
                true,
            )
        }
        "CHANCE_WIN" => (
            "logic.api.gamelogic.chance.Chance_Action",
            "player_choice",
            &state["field"],
            false,
        ),
        "PENALTY" => (
            "logic.api.gamelogic.penalty.Penalty_Action",
            "playerChoice",
            &state["field"],
            false,
        ),
        "PYRAMID_WIN" => {
            let level = state["level"].as_i64().context("missing pyramid level")?;
            if level == 0 {
                return Ok((
                    "logic.api.gamelogic.pyramid.Pyramid_Action",
                    "player_choice",
                    1,
                ));
            }
            if !(level == 1) {
                return Err(crate::model::unknown_event(
                    "unknown pyramid level".to_string(),
                ));
            };
            let row = state["row_in_pyramid"]
                .as_u64()
                .context("missing pyramid row")? as usize;
            (
                "logic.api.gamelogic.pyramid.Pyramid_Action",
                "player_choice",
                &state["pyramid_fields"][row],
                false,
            )
        }
        _ => return Err(crate::model::unknown_event("unknown choice".to_string())),
    };
    let a = cells.as_array().context("missing choice options")?;
    let choice = if hidden {
        a.iter().position(|v| {
            v.as_i64().is_some_and(|n| {
                if fid == "PICKME_EXTENDED" {
                    n == -1
                } else {
                    n < 0
                }
            })
        })
    } else if !a.is_empty() {
        Some(0)
    } else {
        None
    };
    Ok((kind, field, choice.context("no offered choice")?))
}
async fn invoke(
    ctx: &RunContext,
    socket: &mut crate::context::NativeSocket,
    n: &mut u64,
    method: &str,
    payload: Value,
    round_action: bool,
) -> Result<Value> {
    *n += 1;
    let request =
        json!({"@type":"MethodInvocation","requestId":*n,"method":method,"payload":payload});
    let response = ctx
        .ws_json(socket, method, &request, round_action, Some(&n.to_string()))
        .await?;
    let payload = response
        .get("payload")
        .ok_or_else(|| crate::model::unknown_event("missing invocation payload"))?
        .clone();
    ensure!(
        payload["@type"] != "InvocationError",
        "provider invocation error: {}",
        payload["code"]
    );
    Ok(payload)
}
fn valid_https(url: &Url, hosts: &[&str]) -> bool {
    url.scheme() == "https"
        && hosts.contains(&url.host_str().unwrap_or(""))
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
}
async fn get_official_play(ctx: &RunContext, mut url: Url) -> Result<String> {
    let path_re = Regex::new(r"^/(?:es/juegos|games)/[a-z0-9-]+/play$")?;
    for _ in 0..5 {
        ensure!(
            valid_https(&url, &["endorphina.com"])
                && path_re.is_match(url.path())
                && url.query().is_none()
                && url.fragment().is_none(),
            "invalid official demo play URL"
        );
        let response = ctx.get("official_play", url.as_str()).await?;
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            let location = response
                .headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("location"))
                .map(|(_, value)| value)
                .context("official play redirect missing location")?;
            url = url.join(location)?;
            continue;
        }
        ensure!(
            (200..300).contains(&response.status),
            "official play HTTP error"
        );
        return Ok(response.body);
    }
    bail!("too many official play redirects")
}
async fn get_follow(ctx: &RunContext, label: &str, mut url: Url) -> Result<(Url, String)> {
    for _ in 0..5 {
        ensure!(
            valid_https(&url, &["edemo.endorphina.com", "demo.endorphina.network"]),
            "unexpected demo redirect host"
        );
        let r = ctx.get(label, url.as_str()).await?;
        if (300..400).contains(&r.status) {
            let location = r
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("location"))
                .map(|(_, v)| v)
                .context("redirect missing location")?;
            url = url.join(location)?;
            continue;
        }
        ensure!((200..300).contains(&r.status), "demo HTTP error");
        return Ok((url, r.body));
    }
    bail!("too many demo redirects")
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    match mode.kind {
        ModeKind::Normal => ensure!(mode.key == "normal", "invalid normal selector"),
        ModeKind::Purchase => ensure!(
            mode.key.starts_with("buy-") && mode.key.len() > 4,
            "invalid buy selector"
        ),
        _ => bail!("unsupported Endorphina mode"),
    }
    let play = Url::parse(
        settings["launch_url"]
            .as_str()
            .context("Endorphina requires settings.launch_url with the official game play URL")?,
    )?;
    let landing = get_official_play(ctx, play.clone()).await?;
    let re = Regex::new(r"window\.location\.href\s*=\s*'([^']+)'")?;
    let launcher = Url::parse(&re.captures(&landing).context("missing official launcher")?[1])?;
    ensure!(
        valid_https(&launcher, &["edemo.endorphina.com"]),
        "unexpected official launcher"
    );
    let (demo_url, _) = get_follow(ctx, "demo_launcher", launcher).await?;
    ensure!(
        demo_url.host_str() == Some("demo.endorphina.network"),
        "unexpected final demo host"
    );
    let pairs: Vec<(String, String)> = demo_url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let unique = |name: &str| -> Result<String> {
        let mut it = pairs.iter().filter(|(k, _)| k == name);
        let v = it
            .next()
            .context("missing demo launch parameter")?
            .1
            .clone();
        ensure!(
            it.next().is_none() && !v.is_empty(),
            "invalid demo launch parameter"
        );
        Ok(v)
    };
    let launch = unique("launch")?;
    ensure!(
        launch == "https://demo.endorphina.network/organic/websocket/launch",
        "unexpected launch endpoint"
    );
    let session = unique("session")?;
    let sign = unique("sign")?;
    let info = format!(
        "UA={}&L=en-US&V=1280x720x1&R=1280x720&TZ=GMT-0300&LS=[\"en-US\"]&OS=Win 10.0&ARCH=intel&AR=1&DP=12",
        ctx.user_agent()
    );
    let encoded_info: String = info
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("session", &session)
        .append_pair("sign", &sign)
        .append_pair("info", &encoded_info)
        .append_pair("mobile", "false")
        .finish();
    let mut headers = BTreeMap::new();
    headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    let response = ctx
        .http("launch", "POST", &launch, headers, body, false)
        .await?;
    ensure!((200..300).contains(&response.status), "launch HTTP error");
    let launched: Value = serde_json::from_str(&response.body)?;
    let native_game = launched["game"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| launched["game"].as_u64().map(|n| n.to_string()))
        .context("missing native game")?;
    let native_name = launched["gameName"]
        .as_str()
        .context("missing native game name")?;
    ensure!(
        native_name.eq_ignore_ascii_case(&game.name),
        "launched game name differs from inventory"
    );
    if native_game.bytes().all(|b| b.is_ascii_digit()) {
        ensure!(
            native_game == game.id,
            "launched game id differs from inventory"
        );
    }
    let native_re = Regex::new(r"^endorphina2_[A-Za-z0-9_]+$")?;
    if !(native_re.is_match(&native_game)) {
        return Err(crate::model::unknown_event(
            "unrecognized published native game path".to_string(),
        ));
    };
    let config_url = format!(
        "https://demo.endorphina.network/games-wgl/endorphina2/{}/configs/config.desktop.json",
        &native_game["endorphina2_".len()..]
    );
    let config_response = ctx.get("game_feature_config", &config_url).await?;
    ensure!(
        (200..300).contains(&config_response.status),
        "feature configuration HTTP error"
    );
    let config: Value = serde_json::from_str(&config_response.body)?;
    ensure!(
        config["game"]["features"].is_array(),
        "missing published feature configuration"
    );
    let currency = launched["currency"].clone();
    let mut endpoint = Url::parse(
        launched["endpoint"]
            .as_str()
            .context("missing websocket endpoint")?,
    )?;
    let endpoint_re = Regex::new(r"^/organic/websocket/endpoint/[0-9]+$")?;
    ensure!(
        endpoint.scheme() == "wss"
            && endpoint.host_str() == Some("dyndemo.endorphina.network")
            && endpoint.port().is_none()
            && endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.query().is_none()
            && endpoint.fragment().is_none()
            && endpoint_re.is_match(endpoint.path()),
        "unexpected websocket endpoint"
    );
    let ws_session = launched["session"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("missing websocket session")?;
    let ws_sign = launched["sign"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("missing websocket signature")?;
    endpoint
        .query_pairs_mut()
        .append_pair("session", ws_session)
        .append_pair("sign", ws_sign);
    let mut handshake = endpoint.clone();
    handshake
        .set_scheme("https")
        .map_err(|_| anyhow::anyhow!("invalid handshake URL"))?;
    let r = ctx.get("websocket_handshake", handshake.as_str()).await?;
    ensure!((200..300).contains(&r.status), "handshake HTTP error");
    let mut socket = ctx
        .websocket(
            endpoint.as_str(),
            "https://demo.endorphina.network",
            Some("json"),
            BTreeMap::new(),
        )
        .await?;
    let mut n = 0;
    let table = invoke(
        ctx,
        &mut socket,
        &mut n,
        "GET_PAYTABLE_INFO",
        Value::Null,
        false,
    )
    .await?;
    let history = invoke(
        ctx,
        &mut socket,
        &mut n,
        "GET_LAST_GAME",
        Value::Null,
        false,
    )
    .await?;
    let mut p = parameters(&table, &history)?;
    if mode.kind == ModeKind::Purchase {
        let buy = &mode.key[4..];
        let offers = table["buyIns"]
            .as_array()
            .context("missing native buy offers")?;
        ensure!(
            offers.iter().any(|offer| offer["id"] == buy),
            "inventory buy not offered by fresh session"
        );
        p["buyInId"] = json!(buy);
    }
    ctx.save_session(&json!({"provider":"endorphina","game":game.id,"native_game":native_game,"currency":currency,"endpoint":endpoint.as_str(),"play_url":play.as_str()}))?;
    let start = invoke(ctx, &mut socket, &mut n, "START_GAME", p.clone(), true).await?;
    let mut current = start["data"].clone();
    ensure!(current.is_object(), "missing started history");
    let mut trace = vec![json!({"method":"START_GAME","payload":start})];
    for _ in 0..ctx.max_steps() {
        let Some((method, payload)) = next_step(&current, &config)? else {
            break;
        };
        let result = invoke(ctx, &mut socket, &mut n, method, payload.clone(), true).await?;
        trace.push(json!({"method":method,"request_payload":payload,"result":result}));
        current = match method {
            "START_FEATURE" => {
                invoke(
                    ctx,
                    &mut socket,
                    &mut n,
                    "GET_LAST_GAME",
                    Value::Null,
                    false,
                )
                .await?
            }
            "STEP_FEATURE" => result["historyItem"].clone(),
            "END_FEATURE" => result,
            "END_GAME" => result["data"].clone(),
            _ => unreachable!(),
        };
        if !(current.is_object()) {
            return Err(crate::model::unknown_event(
                "missing continuation history".to_string(),
            ));
        };
    }
    ensure!(
        current["status"] == "FINISHED",
        "round did not close within step limit"
    );
    let native_round_id = None;
    Ok(Completion {
        native_round_id,
        details: json!({"launch":launched,"paytable":table,"feature_config":config,"initial_history":history,"parameters":p,"final_history":current,"steps":trace}),
    })
}

pub fn supports(_game: &Game, mode: &Mode) -> bool {
    matches!((mode.kind, mode.key.as_str()), (ModeKind::Normal, "normal"))
        || (mode.kind == ModeKind::Purchase && mode.key.starts_with("buy-") && mode.key.len() > 4)
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn native_finished_history_without_wallet_completes() {
        assert!(
            next_step(&json!({"status":"FINISHED"}), &Value::Null)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn unknown_feature_is_typed_novelty() {
        let error=next_step(&json!({"status":"IN_PROGRESS","features":[{"status":"IN_PROGRESS","feature_id":"NEW_GAME","current_state":{}}]}),&Value::Null).unwrap_err();
        assert!(error.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
}
