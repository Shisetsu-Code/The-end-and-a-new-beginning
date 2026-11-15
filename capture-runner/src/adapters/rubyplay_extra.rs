//! Native HTTP capture controls from ENTREGA-CLAUDE's RubyPlay capture.py and requests.
//! The binary response is opaque: no payout decoder or wallet gate is needed for capture.
//! Only exact game/version pairs and two observed purchase entry bodies are enabled.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::LazyLock, time::Duration};
use url::Url;

const API: &str = "https://srv.prrpeu3.com";
const MAX_EXACT_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Deserialize, Serialize)]
struct Source {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct Purchase {
    bet: u64,
    price: u64,
}

#[derive(Debug, Deserialize)]
struct Binding {
    game_id: u64,
    protocol: u64,
    math: u64,
    source_examples: Vec<String>,
    purchases: BTreeMap<String, Purchase>,
}

#[derive(Deserialize)]
struct Bindings {
    sources: Vec<Source>,
    games: BTreeMap<String, Binding>,
}

static CONTROLS: LazyLock<Bindings> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../bindings/rubyplay-extra.json"))
        .expect("embedded RubyPlay HTTP controls must be valid")
});

fn binding(game: &Game, mode: &Mode) -> Option<&'static Binding> {
    let value = CONTROLS.games.get(&game.id)?;
    match mode.kind {
        ModeKind::Normal if mode.key == "normal" => Some(value),
        ModeKind::Purchase => value
            .purchases
            .contains_key(mode.key.strip_prefix("buy_feature:")?)
            .then_some(value),
        _ => None,
    }
}

pub(super) fn supports(game: &Game, mode: &Mode) -> bool {
    binding(game, mode).is_some()
}

fn integer(value: &Value, label: &str) -> Result<u64> {
    value
        .as_u64()
        .filter(|n| *n <= MAX_EXACT_INTEGER)
        .ok_or_else(|| unknown_event(format!("RubyPlay invalid {label}")))
}

fn accepted(value: &Value, expected: &str, label: &str) -> Result<()> {
    match value["status"].as_str() {
        Some(status) if status == expected => Ok(()),
        Some("error") => bail!("RubyPlay rejected {label}"),
        status => Err(unknown_event(format!(
            "RubyPlay unknown status for {label}: {status:?}"
        ))),
    }
}

#[derive(Debug)]
struct Owner {
    game: u64,
    brand: u64,
}

impl Owner {
    fn from_issuer(value: &Value, binding: &Binding) -> Result<Self> {
        let game = integer(&value["gameId"], "session gameId")?;
        let brand = integer(&value["brandId"], "session brandId")?;
        if !value.is_object() || game != binding.game_id || brand == 0 {
            return Err(unknown_event("RubyPlay session owner changed"));
        }
        Ok(Self { game, brand })
    }

    fn check(&self, value: &Value) -> Result<()> {
        if !value.is_object()
            || value["gameId"].as_u64() != Some(self.game)
            || value["brandId"].as_u64() != Some(self.brand)
        {
            return Err(unknown_event("RubyPlay response session owner changed"));
        }
        Ok(())
    }
}

fn binary_shape(value: &Value) -> Result<()> {
    let encoded = value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256 * 1024)
        .ok_or_else(|| unknown_event("RubyPlay binary response missing or oversized"))?;
    STANDARD
        .decode(encoded)
        .map_err(|e| unknown_event(format!("RubyPlay binary base64 format changed: {e}")))?;
    Ok(())
}

fn envelope<'a>(
    reply: &'a Value,
    action: &str,
    sequence: u64,
    owner: &Owner,
    binding: &Binding,
) -> Result<&'a str> {
    accepted(reply, "ok", action)?;
    if reply["topic"].as_str() != Some(format!("gameserver/{action}").as_str())
        || !reply["data"].is_object()
    {
        return Err(unknown_event("RubyPlay response envelope changed"));
    }
    for key in ["v_protocol", "v_math"] {
        let expected = if key == "v_protocol" {
            binding.protocol
        } else {
            binding.math
        };
        for value in [&reply[key], &reply["data"][key]] {
            if !value.is_null() && value.as_u64() != Some(expected) {
                return Err(unknown_event(format!("RubyPlay response {key} changed")));
            }
        }
    }
    if integer(&reply["data"]["an"], "response sequence")? != sequence {
        return Err(unknown_event("RubyPlay response sequence changed"));
    }
    owner.check(&reply["funModeData"])?;
    binary_shape(&reply["data"]["binary"])?;
    let next = match reply["data"]["next_action"].as_str() {
        Some(action @ ("spin" | "freespin" | "respin" | "minispin" | "pick" | "select")) => action,
        action => {
            return Err(unknown_event(format!(
                "RubyPlay unknown next_action: {action:?}"
            )));
        }
    };
    if (action == "init" || next != "spin")
        && !reply["funModeData"]["sessionData"]
            .as_str()
            .is_some_and(|state| !state.trim().is_empty())
    {
        return Err(unknown_event(
            "RubyPlay opaque sessionData missing before a next action",
        ));
    }
    Ok(next)
}

#[derive(Debug)]
struct State<'a> {
    binding: &'a Binding,
    key: String,
    owner: Owner,
    fun: Value,
    an: u64,
    next: String,
    bet: u64,
    purchase: Option<(&'a str, &'a Purchase)>,
    steps: usize,
}

impl<'a> State<'a> {
    fn from_init(
        binding: &'a Binding,
        mode: &'a Mode,
        issuer: &Value,
        init: &Value,
    ) -> Result<Self> {
        accepted(issuer, "success", "demo session")?;
        let key = issuer["sessionKey"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| unknown_event("RubyPlay demo key missing"))?
            .to_owned();
        let owner = Owner::from_issuer(&issuer["funModeData"], binding)?;
        let next = envelope(init, "init", 0, &owner, binding)?;
        if next != "spin" {
            return Err(unknown_event("RubyPlay fresh init is not idle"));
        }
        let bets = init["data"]["game_config"]["bets"]
            .as_array()
            .filter(|values| !values.is_empty())
            .ok_or_else(|| unknown_event("RubyPlay native bet menu missing"))?
            .iter()
            .map(|value| {
                let bet = integer(value, "offered bet")?;
                if bet == 0 {
                    return Err(unknown_event("RubyPlay offered a nonpositive bet"));
                }
                Ok(bet)
            })
            .collect::<Result<Vec<_>>>()?;
        let purchase = if let Some(feature) = mode.key.strip_prefix("buy_feature:") {
            Some((
                feature,
                binding
                    .purchases
                    .get(feature)
                    .context("RubyPlay purchase has no exact entry contract")?,
            ))
        } else {
            None
        };
        let bet = if let Some((feature, control)) = purchase {
            ensure!(
                init["data"]["buy_feature_available"] == true,
                "RubyPlay requested purchase is unavailable"
            );
            let offers = init["data"]["buy_feature_configs"]
                .as_array()
                .ok_or_else(|| unknown_event("RubyPlay purchase menu format changed"))?;
            let found = offers
                .iter()
                .filter(|offer| offer["buyFeatureType"] == feature)
                .count();
            ensure!(found > 0, "RubyPlay requested purchase is not offered");
            if found != 1 {
                return Err(unknown_event("RubyPlay purchase selector is ambiguous"));
            }
            ensure!(
                bets.contains(&control.bet),
                "RubyPlay documented purchase bet is not offered"
            );
            control.bet
        } else {
            *bets.iter().min().expect("nonempty native bet menu")
        };
        Ok(Self {
            binding,
            key,
            owner,
            fun: init["funModeData"].clone(),
            an: 0,
            next: next.to_owned(),
            bet,
            purchase,
            steps: 0,
        })
    }

    fn request(&self) -> Result<Value> {
        let action = if self.steps == 0 {
            if self.purchase.is_some() {
                "buy_feature"
            } else {
                "spin"
            }
        } else {
            match self.next.as_str() {
                action @ ("freespin" | "respin" | "minispin") => action,
                action => {
                    return Err(unknown_event(format!(
                        "RubyPlay has no reviewed continuation parameters for {action}"
                    )));
                }
            }
        };
        let mut request = json!({
            "v_protocol": self.binding.protocol,
            "v_math": self.binding.math,
            "key": self.key,
            "device_type": "desktop",
            "funModeData": self.fun,
            "an": self.an,
            "action": action,
        });
        if self.steps == 0 {
            request["bet"] = json!(self.bet);
        }
        if let Some((feature, control)) = self.purchase {
            request["buy_feature_type"] = json!(feature);
            if self.steps == 0 {
                // Literal observed entry price at its exact bet; no factor or wallet arithmetic.
                request["buy_feature_price"] = json!(control.price);
            }
        }
        Ok(request)
    }

    fn advance(&mut self, action: &str, reply: &Value) -> Result<bool> {
        let next_an = self
            .an
            .checked_add(1)
            .filter(|value| *value <= MAX_EXACT_INTEGER)
            .ok_or_else(|| unknown_event("RubyPlay sequence overflow"))?;
        let next = envelope(reply, action, next_an, &self.owner, self.binding)?;
        if let Some((feature, control)) = self.purchase {
            if action == "buy_feature"
                && (reply["data"]["buy_feature_type"] != feature
                    || reply["data"]["buy_feature_price"].as_u64() != Some(control.price))
            {
                return Err(unknown_event("RubyPlay purchase response changed"));
            }
            if reply["data"].get("buy_feature_type").is_some()
                && reply["data"]["buy_feature_type"] != feature
            {
                return Err(unknown_event("RubyPlay purchase continuation changed"));
            }
        } else if reply["data"].get("buy_feature_type").is_some() {
            return Err(unknown_event(
                "RubyPlay normal response gained a purchase selector",
            ));
        }
        self.fun = reply["funModeData"].clone();
        self.an = next_an;
        self.next = next.to_owned();
        self.steps += 1;
        Ok(next == "spin")
    }
}

pub(super) async fn execute(ctx: &RunContext, game: &Game, mode: &Mode) -> Result<Completion> {
    let binding = binding(game, mode).context("RubyPlay mode has no explicit HTTP controls")?;
    let headers = BTreeMap::from([
        ("user-agent".into(), ctx.user_agent().into()),
        ("accept".into(), "application/json".into()),
    ]);
    let url = Url::parse_with_params(
        &format!("{API}/init-session/demo"),
        [
            ("gamename", game.id.as_str()),
            ("operator", "rubyplay.com"),
            ("mode", "fun"),
            ("currency", "EUR"),
            ("playerSession", ""),
        ],
    )?;
    let issuer = ctx
        .http(
            "demo_session",
            "GET",
            url.as_str(),
            headers.clone(),
            String::new(),
            false,
        )
        .await?;
    ensure!(
        issuer.status == 200,
        "RubyPlay demo session HTTP {}",
        issuer.status
    );
    let issuer: Value = serde_json::from_str(&issuer.body)?;
    accepted(&issuer, "success", "demo session")?;
    let key = issuer["sessionKey"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| unknown_event("RubyPlay demo key missing"))?;
    Owner::from_issuer(&issuer["funModeData"], binding)?;
    let endpoint = format!("{API}/gameserver/demo");
    tokio::time::sleep(Duration::from_millis(800)).await;
    let init = ctx
        .json(
            "init",
            &endpoint,
            &json!({"v_protocol":binding.protocol,"v_math":binding.math,"key":key,
                "device_type":"desktop","funModeData":issuer["funModeData"],"action":"init"}),
            headers.clone(),
            false,
        )
        .await?;
    let mut state = State::from_init(binding, mode, &issuer, &init)?;
    ctx.save_session(
        &json!({"game":game.id,"key":key,"v_protocol":binding.protocol,"v_math":binding.math}),
    )?;
    ctx.note(
        "rubyplay_http_controls",
        &json!({"sources":CONTROLS.sources,
        "source_examples":binding.source_examples,"binary_interpretation":"opaque",
        "closure":"native next_action=spin with consecutive an"}),
    )?;
    for _ in 0..ctx.max_steps() {
        let request = state.request()?;
        let action = request["action"].as_str().expect("constructed action");
        tokio::time::sleep(Duration::from_millis(800)).await;
        let reply = ctx
            .json(action, &endpoint, &request, headers.clone(), true)
            .await?;
        if state.advance(action, &reply)? {
            return Ok(Completion {
                native_round_id: None,
                details: json!({"game":game.id,"mode":mode.key,"actions":state.steps,
                    "last_an":state.an,"v_protocol":binding.protocol,"v_math":binding.math,
                    "binary_interpretation":"opaque","native_terminal":"next_action=spin"}),
            });
        }
    }
    Err(unknown_event("RubyPlay continuation limit reached"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UnknownEvent;

    fn mode(key: &str) -> Mode {
        Mode {
            key: key.into(),
            kind: if key == "normal" {
                ModeKind::Normal
            } else {
                ModeKind::Purchase
            },
            label: key.into(),
            requires_prerequisites: Value::Null,
        }
    }

    fn session(game: u64) -> Value {
        json!({"status":"success","sessionKey":"test-new-session",
            "funModeData":{"brandId":427,"gameId":game,"opaque_source":"issuer"}})
    }

    fn reply(game: u64, action: &str, an: u64, next: &str) -> Value {
        json!({"status":"ok","topic":format!("gameserver/{action}"),
            "data":{"an":an,"next_action":next,"binary":"AAEC"},
            "funModeData":{"brandId":427,"gameId":game,"sessionData":format!("test-state-{an}")}})
    }

    fn init(game: u64) -> Value {
        let mut value = reply(game, "init", 0, "spin");
        value["data"]["game_config"] = json!({"bets":[10,1,5]});
        value["data"]["buy_feature_available"] = json!(true);
        value["data"]["buy_feature_configs"] =
            json!([{"buyFeatureType":"freespin","priceMultiplier":50}]);
        value
    }

    fn novelty(error: anyhow::Error) {
        assert!(error.downcast_ref::<UnknownEvent>().is_some(), "{error:#}");
    }

    #[test]
    fn normal_minimum_offer_and_native_close_need_no_balance_or_payout_decoder() {
        let binding = &CONTROLS.games["rp_1"];
        let selected = mode("normal");
        let issuer = session(1);
        let initial = init(1);
        let mut state = State::from_init(binding, &selected, &issuer, &initial).unwrap();
        assert_eq!(
            state.request().unwrap(),
            json!({"bet":1,"v_protocol":2,"v_math":20190114,
            "an":0,"key":"test-new-session","device_type":"desktop","funModeData":initial["funModeData"],"action":"spin"})
        );
        assert!(state.advance("spin", &reply(1, "spin", 1, "spin")).unwrap());
        assert_eq!(state.steps, 1);
    }

    #[test]
    fn purchase_uses_exact_observed_fields_and_carries_native_state() {
        for game in [102, 103] {
            let binding = &CONTROLS.games[&format!("rp_{game}")];
            let selected = mode("buy_feature:freespin");
            let mut state =
                State::from_init(binding, &selected, &session(game), &init(game)).unwrap();
            let request = state.request().unwrap();
            assert_eq!(request["bet"], 1);
            assert_eq!(request["buy_feature_price"], 500);
            assert_eq!(request["buy_feature_type"], "freespin");
            assert_eq!(request["action"], "buy_feature");
            let mut accepted = reply(game, "buy_feature", 1, "freespin");
            accepted["data"]["buy_feature_type"] = json!("freespin");
            accepted["data"]["buy_feature_price"] = json!(500);
            assert!(!state.advance("buy_feature", &accepted).unwrap());
            let continuation = state.request().unwrap();
            assert_eq!(continuation["action"], "freespin");
            assert_eq!(continuation["an"], 1);
            assert_eq!(continuation["funModeData"], accepted["funModeData"]);
            assert_eq!(continuation["buy_feature_type"], "freespin");
            assert!(continuation.get("bet").is_none());
            assert!(continuation.get("buy_feature_price").is_none());
            assert!(
                state
                    .advance("freespin", &reply(game, "freespin", 2, "spin"))
                    .unwrap()
            );
        }
    }

    #[test]
    fn consecutive_free_respin_and_minispin_are_the_only_automatic_continuations() {
        let selected = mode("normal");
        let mut state = State::from_init(
            &CONTROLS.games["rp_106"],
            &selected,
            &session(106),
            &init(106),
        )
        .unwrap();
        for (sequence, action, next) in [
            (1, "spin", "respin"),
            (2, "respin", "freespin"),
            (3, "freespin", "minispin"),
            (4, "minispin", "spin"),
        ] {
            assert_eq!(state.request().unwrap()["action"], action);
            assert_eq!(
                state
                    .advance(action, &reply(106, action, sequence, next))
                    .unwrap(),
                next == "spin"
            );
        }
    }

    #[test]
    fn choice_does_not_guess_indices_or_retry_actions() {
        let selected = mode("normal");
        for next in ["pick", "select"] {
            let mut state = State::from_init(
                &CONTROLS.games["rp_112"],
                &selected,
                &session(112),
                &init(112),
            )
            .unwrap();
            assert!(!state.advance("spin", &reply(112, "spin", 1, next)).unwrap());
            novelty(state.request().unwrap_err());
            assert_eq!(state.an, 1);
            assert_eq!(state.steps, 1);
        }
    }

    #[test]
    fn malformed_fresh_session_is_novelty_before_any_bet() {
        let selected = mode("normal");
        let binding = &CONTROLS.games["rp_109"];
        for path in ["an", "next_action", "binary"] {
            let mut value = init(109);
            value["data"][path] = match path {
                "an" => json!(1),
                "next_action" => json!("freespin"),
                _ => json!("not base64"),
            };
            novelty(State::from_init(binding, &selected, &session(109), &value).unwrap_err());
        }
        let mut wrong_game = session(109);
        wrong_game["funModeData"]["gameId"] = json!(110);
        novelty(State::from_init(binding, &selected, &wrong_game, &init(109)).unwrap_err());
    }

    #[test]
    fn carried_native_state_is_required_before_another_request_not_after_close() {
        let selected = mode("normal");
        let binding = &CONTROLS.games["rp_1"];
        for invalid in [Value::Null, json!(""), json!("  "), json!({"an":0})] {
            let mut initial = init(1);
            initial["funModeData"]["sessionData"] = invalid.clone();
            novelty(State::from_init(binding, &selected, &session(1), &initial).unwrap_err());
            let mut state = State::from_init(binding, &selected, &session(1), &init(1)).unwrap();
            let mut open = reply(1, "spin", 1, "respin");
            open["funModeData"]["sessionData"] = invalid;
            novelty(state.advance("spin", &open).unwrap_err());
            assert_eq!(state.steps, 0);
            assert_eq!(state.an, 0);
        }
        let mut state = State::from_init(binding, &selected, &session(1), &init(1)).unwrap();
        let mut closed = reply(1, "spin", 1, "spin");
        closed["funModeData"]
            .as_object_mut()
            .unwrap()
            .remove("sessionData");
        assert!(state.advance("spin", &closed).unwrap());
    }

    #[test]
    fn unknown_state_sequence_format_or_version_stops_without_advancing() {
        let selected = mode("normal");
        let good = reply(1, "spin", 1, "spin");
        let mutations = [
            ("/data/next_action", json!("new_bonus")),
            ("/data/an", json!(0)),
            ("/data/an", json!(2)),
            ("/data/binary", Value::Null),
            ("/data/binary", json!("!")),
            ("/topic", json!("gameserver/new_spin")),
            ("/funModeData/gameId", json!(2)),
            ("/funModeData/brandId", json!(428)),
            ("/status", json!("new_status")),
        ];
        for (path, value) in mutations {
            let mut state =
                State::from_init(&CONTROLS.games["rp_1"], &selected, &session(1), &init(1))
                    .unwrap();
            let mut changed = good.clone();
            *changed.pointer_mut(path).unwrap() = value;
            novelty(state.advance("spin", &changed).unwrap_err());
            assert_eq!(state.an, 0);
            assert_eq!(state.steps, 0);
        }
        let mut state =
            State::from_init(&CONTROLS.games["rp_1"], &selected, &session(1), &init(1)).unwrap();
        let mut changed = good;
        changed["data"]["v_math"] = json!(0);
        novelty(state.advance("spin", &changed).unwrap_err());
    }

    #[test]
    fn rejected_action_stays_rejected_and_purchase_selectors_cannot_drift() {
        let selected = mode("buy_feature:freespin");
        let binding = &CONTROLS.games["rp_102"];
        let mut state = State::from_init(binding, &selected, &session(102), &init(102)).unwrap();
        let rejection = state
            .advance(
                "buy_feature",
                &json!({"status":"error","data":{"code":"M-0"}}),
            )
            .unwrap_err();
        assert!(rejection.downcast_ref::<UnknownEvent>().is_none());
        assert_eq!(state.steps, 0);
        let mut changed = reply(102, "buy_feature", 1, "freespin");
        changed["data"]["buy_feature_type"] = json!("freespin");
        changed["data"]["buy_feature_price"] = json!(501);
        novelty(state.advance("buy_feature", &changed).unwrap_err());
        let mut unavailable = init(102);
        unavailable["data"]["buy_feature_configs"] = json!([]);
        assert!(State::from_init(binding, &selected, &session(102), &unavailable).is_err());
        unavailable["data"]["buy_feature_configs"] =
            json!([{"buyFeatureType":"freespin"},{"buyFeatureType":"freespin"}]);
        novelty(State::from_init(binding, &selected, &session(102), &unavailable).unwrap_err());
    }
}
