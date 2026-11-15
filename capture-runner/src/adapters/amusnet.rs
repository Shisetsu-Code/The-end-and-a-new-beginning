//! Native capture connector derived from standalone info/amusnet/codigo/amusnet.rs.
//! Published source SHA-256: 31374690d1ce02c04a8b5984329835e0e546e01d3212f8b3af327972e2aec524
//! Reference transport: game-runner/src/adapters/amusnet.rs; no runtime dependency.
use crate::{
    context::{NativeSocket, RunContext, SocketPayload},
    model::{Completion, Game, Mode, decimal, exact_mul, now_ms},
};
use anyhow::{Context, Result, bail, ensure};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock};
use url::Url;
use uuid::Uuid;

const PORTAL: &str = "https://amusnet.com";
const HOST: &str = "free-pod.games.amusnet.io";

fn binding(game: &Game, mode: &Mode) -> Option<(bool, &'static Value)> {
    static MAP: OnceLock<Value> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        serde_json::from_str(include_str!("../../bindings/amusnet.json"))
            .expect("embedded Amusnet bindings")
    });
    let row = map.get(&game.id)?;
    let selected = row["modes"].get(&mode.key)?;
    (selected["kind"] == mode.kind.as_str())
        .then_some((row["remote"].as_bool()?, &selected["option"]))
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    binding(game, mode).is_some() || dice_binding(game, mode).is_some()
}

fn dice_binding(game: &Game, mode: &Mode) -> Option<&'static Value> {
    static MAP: OnceLock<Value> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        serde_json::from_str(include_str!("../../bindings/amusnet-extra.json"))
            .expect("valid Amusnet dice native controls")
    });
    let row = map["games"].get(&game.id)?.get(&mode.key)?;
    (row["kind"] == mode.kind.as_str()).then_some(row)
}

pub(crate) fn demo_socket_host(host: &str) -> bool {
    host.strip_prefix("free-pod")
        .and_then(|s| s.strip_suffix(".games.amusnet.io"))
        .is_some_and(|digits| digits.bytes().all(|b| b.is_ascii_digit()))
}

fn amount(value: &Value) -> Result<Decimal> {
    match value {
        Value::Number(value) => decimal(&value.to_string()),
        Value::String(value) => decimal(value),
        _ => bail!("Amusnet native amount missing"),
    }
}

fn lowest(bets: &Value) -> Result<Decimal> {
    bets.as_array()
        .context("Amusnet native bet menu missing")?
        .iter()
        .map(amount)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|value| *value > Decimal::ZERO)
        .min()
        .context("Amusnet positive bet option missing")
}

fn quote(settings: &Value, option: &Value, remote: bool) -> Result<Decimal> {
    let group = if let Some(groups) = settings["betOptions"]
        .as_array()
        .filter(|array| !array.is_empty())
    {
        let offers = groups
            .iter()
            .map(|group| Ok((lowest(&group["betAmounts"])?, group)))
            .collect::<Result<Vec<_>>>()?;
        offers.into_iter().min_by_key(|(bet, _)| *bet)
    } else {
        None
    };
    let values = settings
        .get("bets")
        .or_else(|| settings.get("betAmounts"))
        .filter(|value| value.as_array().is_some_and(|array| !array.is_empty()));
    let base = match values {
        Some(values) => lowest(values)?,
        None => group.context("Amusnet bet amounts missing")?.0,
    };
    let name = option["name"]
        .as_str()
        .context("Amusnet option name missing")?;
    let offered_multiplier = if name == "NONE" {
        let expected = group
            .map(|(_, group)| &group["betGroupId"])
            .unwrap_or(&Value::Null);
        ensure!(
            expected == &option["bet_group"],
            "Amusnet native bet group changed"
        );
        Decimal::ONE
    } else if remote {
        ensure!(option["id"] == name, "Amusnet remote selector changed");
        amount(&settings["buyFeatures"][name]["multiplier"])?
    } else {
        let id = &settings["featureIds"][name];
        ensure!(
            id == &option["id"] && id.is_number(),
            "Amusnet native selector changed"
        );
        amount(&settings["featureMultipliers"][id.to_string()])?
    };
    ensure!(
        offered_multiplier == amount(&option["multiplier"])? && offered_multiplier > Decimal::ZERO,
        "Amusnet mode price changed"
    );
    Ok(base)
}

fn dice_option(settings: &Value, key: &str, remote: bool) -> Result<Value> {
    ensure!(
        matches!(key, "NONE" | "BUY_BONUS_GAME"),
        "unsupported Amusnet dice selector"
    );
    let mut option = if key == "NONE" {
        json!({"name":"NONE","id":0,"multiplier":1})
    } else if remote {
        let multiplier = settings["buyFeatures"][key]
            .get("multiplier")
            .context("Amusnet requested dice purchase unavailable")?;
        json!({"name":key,"id":key,"multiplier":multiplier})
    } else {
        let id = settings["featureIds"][key]
            .as_u64()
            .context("Amusnet requested dice purchase unavailable")?;
        json!({"name":key,"id":id,"multiplier":settings["featureMultipliers"][id.to_string()]})
    };
    if key == "NONE"
        && let Some(groups) = settings["betOptions"].as_array().filter(|g| !g.is_empty())
    {
        let offers = groups
            .iter()
            .map(|g| Ok((lowest(&g["betAmounts"])?, g)))
            .collect::<Result<Vec<_>>>()?;
        if let Some((_, group)) = offers.into_iter().min_by_key(|(bet, _)| *bet) {
            option["bet_group"] = group["betGroupId"].clone();
        }
    }
    ensure!(
        amount(&option["multiplier"])? > Decimal::ZERO,
        "invalid native Amusnet dice multiplier"
    );
    Ok(option)
}

fn dice_entry(option: &Value, base: Decimal, remote: bool) -> Result<Value> {
    let native = bet_request("", option, base, remote, false)?;
    Ok(json!({"bet":native["bet"],"featureName":option["name"]}))
}

fn has_offer(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Bool(true) => true,
    }
}

fn dice_continuation(
    state: &str,
    reply: &Value,
    settings: &Value,
) -> Result<(&'static str, Value)> {
    if state == "player_move" {
        return Ok(("MOVE", json!({"box":-1})));
    }
    if state != "bonus_game" {
        return Err(crate::model::unknown_event(format!(
            "Amusnet unknown dice state {state}"
        )));
    }
    let bonus = reply["complex"]["bonusGame"]
        .as_object()
        .ok_or_else(|| crate::model::unknown_event("Amusnet dice bonus control missing"))?;
    if bonus.get("offeredPrize").is_some_and(has_offer) || bonus.get("prize").is_some_and(has_offer)
    {
        return Ok(("BONUS_PICK_DECISION", json!({"accepted":true})));
    }
    let count = settings["bonusPrizes"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| crate::model::unknown_event("Amusnet dice prize menu missing"))?
        .len();
    let rejected = match bonus.get("rejectedPrizes") {
        None => &[][..],
        Some(Value::Array(items)) => items.as_slice(),
        _ => {
            return Err(crate::model::unknown_event(
                "Amusnet dice rejected-prize format changed",
            ));
        }
    };
    let mut used = std::collections::BTreeSet::new();
    for item in rejected {
        let index = item["index"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| *v < count)
            .ok_or_else(|| {
                crate::model::unknown_event("Amusnet dice rejected-prize index invalid")
            })?;
        if !used.insert(index) {
            return Err(crate::model::unknown_event(
                "Amusnet dice rejected-prize index duplicated",
            ));
        }
    }
    let index = (0..count).find(|i| !used.contains(i)).ok_or_else(|| {
        crate::model::unknown_event("Amusnet dice has no unselected native prize")
    })?;
    Ok(("BONUS_PICK", json!({"index":index})))
}

struct Session<'a> {
    ctx: &'a RunContext,
    socket: NativeSocket,
    key: &'a str,
    game: &'a str,
    number: Value,
    remote: bool,
}

impl Session<'_> {
    async fn call(
        &mut self,
        command: &str,
        mut request: Value,
        round_action: bool,
    ) -> Result<Value> {
        let id = format!("qa-{}", Uuid::new_v4());
        request["command"] = json!(command);
        request["gin"] = json!(self.game);
        request["sessionKey"] = json!(self.key);
        request["messageId"] = json!(id);
        self.ctx
            .ws_codec(
                &mut self.socket,
                command,
                SocketPayload::Text(request.to_string()),
                round_action,
                None,
                |frame| {
                    let SocketPayload::Text(text) = frame else {
                        return Err(crate::model::unknown_event(
                            "Amusnet unexpected binary frame".to_string(),
                        ));
                    };
                    if text == "pong" {
                        return Ok(Vec::new());
                    }
                    let answer: Value = serde_json::from_str(text)?;
                    Ok(if answer["messageId"] == id {
                        vec![answer]
                    } else {
                        Vec::new()
                    })
                },
            )
            .await
    }

    async fn action(&mut self, command: &str, mut request: Value) -> Result<Value> {
        let reply = if self.remote {
            request["gameCommand"] = json!(command.to_ascii_uppercase());
            request["gameNumber"] = self.number.clone();
            let outer = self
                .call("forward", json!({"payload":request}), true)
                .await?;
            outer.get("payload").cloned().unwrap_or(outer)
        } else {
            request["gameCommand"] = json!(command);
            self.call("bet", json!({"gameNumber":self.number,"bet":request}), true)
                .await?
        };
        ensure!(
            reply
                .get("msg")
                .or_else(|| reply.get("status"))
                .is_none_or(|value| value == "success"),
            "Amusnet native command rejected"
        );
        Ok(reply)
    }
}

fn bet_request(
    game: &str,
    option: &Value,
    base: Decimal,
    remote: bool,
    free: bool,
) -> Result<Value> {
    let multiplier = if free {
        Decimal::ONE
    } else {
        amount(&option["multiplier"])?
    };
    let stake = exact_mul(base, multiplier)?;
    ensure!(
        stake.fract().is_zero(),
        "Amusnet bet has a fractional native unit"
    );
    let stake = stake
        .normalize()
        .to_string()
        .parse::<u64>()
        .context("Amusnet bet out of range")?;
    let mut request = json!({"bet":stake,"betMultiplier":serde_json::from_str::<Value>(&multiplier.to_string())?,
        "bonus":free,"autoplay":false});
    if remote {
        request["buyFeature"] = if free {
            json!("NONE")
        } else {
            option["name"].clone()
        };
    } else {
        request["featureId"] = if free { json!(0) } else { option["id"].clone() };
    }
    if let Some(group) = option.get("bet_group") {
        request["betGroupId"] = group.clone();
    }
    if matches!(game, "414" | "450") {
        request["gameMode"] = json!(2);
    }
    Ok(request)
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    let bound = binding(game, mode);
    let dice = dice_binding(game, mode);
    ensure!(
        bound.is_some() || dice.is_some(),
        "Amusnet mode binding missing"
    );
    let headers = BTreeMap::from([
        ("origin".into(), PORTAL.into()),
        ("referer".into(), format!("{PORTAL}/")),
        ("user-agent".into(), "Mozilla/5.0".into()),
    ]);
    let lobby = ctx
        .json(
            "demo_lobby",
            &format!("{PORTAL}/api/lobby"),
            &json!({"timestamp":now_ms()}),
            headers.clone(),
            false,
        )
        .await?;
    let token = lobby["token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .context("Amusnet fresh guest token missing")?;
    let mut launch = Url::parse(&format!("https://{HOST}/demo/v1/launch"))?;
    launch.query_pairs_mut().extend_pairs([
        ("gameId", game.id.as_str()),
        ("language", "en"),
        ("client", "desktop"),
        ("token", token),
    ]);
    let reply = ctx
        .http(
            "demo_launch",
            "GET",
            launch.as_str(),
            headers,
            String::new(),
            false,
        )
        .await?;
    ensure!(
        reply.status == 200,
        "Amusnet launcher HTTP {}",
        reply.status
    );
    let marker = regex::Regex::new(r"\bvar\s+connectionParams\s*=\s*")?
        .find(&reply.body)
        .context("Amusnet connection parameters missing")?;
    let config = serde_json::Deserializer::from_str(&reply.body[marker.end()..])
        .into_iter::<Value>()
        .next()
        .context("Amusnet launcher configuration missing")??;
    let demo_host = config["tcpHost"]
        .as_str()
        .context("Amusnet demo socket host missing")?;
    let remote = match &config["remote"] {
        Value::Bool(v) => *v,
        Value::Null => false,
        _ => {
            return Err(crate::model::unknown_event(
                "Amusnet remote protocol marker changed",
            ));
        }
    };
    ensure!(
        config["portalCode"] == "Corporate_All_Portal_FUN"
            && demo_socket_host(demo_host)
            && bound.is_none_or(|(expected, _)| expected == remote),
        "Amusnet public FUN launcher contract changed"
    );
    let key = config["sessionKey"]
        .as_str()
        .filter(|value| !value.is_empty())
        .context("Amusnet session key missing")?;
    let mut ws = Url::parse(&format!("wss://{demo_host}/"))?;
    ws.query_pairs_mut().append_pair("sessionKey", key);
    let socket = ctx
        .websocket(
            ws.as_str(),
            "https://prod.cdn.amusnet.io",
            None,
            BTreeMap::new(),
        )
        .await?;
    let mut session = Session {
        ctx,
        socket,
        key,
        game: &game.id,
        number: json!("-1"),
        remote,
    };
    let init = session
        .call("init_game", json!({"platformType":"desktop"}), false)
        .await?;
    ensure!(
        init["status"] == "success",
        "Amusnet did not confirm native initialization"
    );
    let state = &init["states"]["gameState"];
    let current = state
        .pointer("/complex/currentState/state")
        .unwrap_or(&state["state"]);
    ensure!(
        current.as_str().is_some_and(|state| matches!(
            state.to_ascii_lowercase().as_str(),
            "idle" | "finished"
        )),
        "Amusnet demo is not idle"
    );
    if !state["gameNumber"].is_null() {
        session.number = state["gameNumber"].clone();
    }
    let settings = &init["settings"]["gameSettings"];
    let settings = settings.get("complex").unwrap_or(settings);
    let option = if let Some((_, option)) = bound {
        option.clone()
    } else {
        dice_option(settings, &mode.key, remote)?
    };
    let base = quote(settings, &option, remote)?;
    let mut request = if dice.is_some() {
        dice_entry(&option, base, remote)?
    } else {
        bet_request(&game.id, &option, base, remote, false)?
    };
    let cost = amount(&request["bet"])?;
    ctx.save_session(&json!({"provider":"amusnet","game":game.id,"sessionKey":key,"currency":"FUN","mode":mode.key,"cost":cost}))?;
    let mut command = dice.and_then(|v| v["command"].as_str()).unwrap_or("bet");
    let mut root = None;
    for step in 0..ctx.max_steps().min(600) {
        let reply = session.action(command, request).await?;
        if let Some(number) = reply
            .get("gameNumber")
            .filter(|value| value.is_string() || value.is_number())
        {
            ensure!(
                dice.is_some() || root.as_ref().is_none_or(|prior| prior == number),
                "Amusnet native round identity changed"
            );
            root.get_or_insert_with(|| number.clone());
            session.number = number.clone();
        }
        let state = reply["state"]
            .as_str()
            .ok_or_else(|| crate::model::unknown_event("Amusnet native round state missing"))?
            .to_ascii_lowercase();
        if dice.is_some() && matches!(state.as_str(), "player_move" | "bonus_game") {
            (command, request) = dice_continuation(&state, &reply, settings)?;
            continue;
        }
        match known_state(&state)? {
            "idle" | "finished" => {
                return Ok(Completion {
                    native_round_id: root.map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string())
                    }),
                    details: json!({"steps":step+1,"option":option["name"]}),
                });
            }
            "freespin" => {
                command = if remote { "freespin" } else { "bet" };
                request = bet_request(&game.id, &option, base, remote, true)?;
            }
            "gamble" => {
                command = "collect";
                request = json!({});
            }
            "hold_and_spin" if remote => {
                command = "HOLD_AND_SPIN";
                request = json!({"bet":0,"betMultiplier":1,"bonus":false,"buyFeature":"NONE","autoplay":false});
            }
            _ => {
                return Err(crate::model::unknown_event(
                    "Amusnet continuation requires a game-specific control".to_string(),
                ));
            }
        }
    }
    Err(crate::model::unknown_event(
        "Amusnet continuation limit reached",
    ))
}

fn known_state(state: &str) -> Result<&str> {
    match state {
        "idle" | "finished" | "freespin" | "gamble" | "hold_and_spin" => Ok(state),
        _ => Err(crate::model::unknown_event(format!(
            "Amusnet unknown state {state}"
        ))),
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn dice_native_entry_and_choices_need_no_wallet() {
        let settings = json!({"bets":[10,20],"buyFeatures":{"BUY_BONUS_GAME":{"multiplier":50}},"bonusPrizes":[1,2,3]});
        let option = dice_option(&settings, "BUY_BONUS_GAME", true).unwrap();
        assert_eq!(
            dice_entry(&option, quote(&settings, &option, true).unwrap(), true).unwrap(),
            json!({"bet":500,"featureName":"BUY_BONUS_GAME"})
        );
        assert_eq!(
            dice_continuation("player_move", &json!({}), &settings).unwrap(),
            ("MOVE", json!({"box":-1}))
        );
        assert_eq!(
            dice_continuation(
                "bonus_game",
                &json!({"complex":{"bonusGame":{"rejectedPrizes":[{"index":0}]}}}),
                &settings
            )
            .unwrap(),
            ("BONUS_PICK", json!({"index":1}))
        );
        assert_eq!(
            dice_continuation(
                "bonus_game",
                &json!({"complex":{"bonusGame":{"offeredPrize":{"index":1}}}}),
                &settings
            )
            .unwrap(),
            ("BONUS_PICK_DECISION", json!({"accepted":true}))
        );
    }

    #[test]
    fn dice_unknown_control_and_malformed_menu_alert() {
        for reply in [
            json!({}),
            json!({"complex":{"bonusGame":{"rejectedPrizes":[{"index":9}]}}}),
        ] {
            assert!(
                dice_continuation("bonus_game", &reply, &json!({"bonusPrizes":[1]}))
                    .unwrap_err()
                    .downcast_ref::<crate::model::UnknownEvent>()
                    .is_some()
            );
        }
        assert!(dice_continuation("new_event", &json!({}), &json!({})).is_err());
        assert!(demo_socket_host("free-pod12.games.amusnet.io"));
        assert!(!demo_socket_host("free-pod12.games.amusnet.io.example.com"));
        assert!(!demo_socket_host("free-pod-live.games.amusnet.io"));
    }

    #[test]
    fn native_terminal_state_needs_no_wallet_and_unknown_is_typed() {
        let reply = json!({"state":"finished","gameNumber":"r1"});
        assert_eq!(
            known_state(reply["state"].as_str().unwrap()).unwrap(),
            "finished"
        );
        assert!(
            known_state("new_bonus")
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
