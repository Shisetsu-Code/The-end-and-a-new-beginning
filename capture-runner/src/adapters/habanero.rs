//! Native capture protocol derived from standalone info/habanero/codigo and the evidence reference adapter.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, sha256, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock, time::Duration};
use uuid::Uuid;

const API: &str = "https://gs-test.insvr.com/pf?client=1.1.24603.707";

#[derive(Deserialize)]
struct Binding {
    brand: String,
    legacy: bool,
    lines: u64,
    coins: u64,
    modes: BTreeMap<String, String>,
    #[serde(default)]
    sources: BTreeMap<String, PublicSource>,
}

#[derive(Deserialize)]
struct PublicSource {
    url: String,
    sha256: String,
}

fn source_game(id: &str) -> &str {
    match id {
        "SGTabernaDeLosMuertosUltra" => "SGTabernaDeLosMuertos",
        "SGJellyfishFlowUltra" => "SGJellyfishFlow",
        _ => id,
    }
}

fn cake_choice(game: &str, mode: &str) -> Option<u8> {
    if game != "SGCakeValley" {
        return None;
    }
    match mode {
        "normal-1" => Some(1),
        "normal-2" => Some(2),
        "normal-3" => Some(3),
        _ => None,
    }
}

fn binding(game: &Game, mode: &Mode) -> Option<(&'static Binding, &'static str)> {
    static GAMES: OnceLock<BTreeMap<String, Binding>> = OnceLock::new();
    if !matches!(
        (mode.key.as_str(), mode.kind),
        ("normal", ModeKind::Normal)
            | ("superbet", ModeKind::Booster)
            | ("buy", ModeKind::Purchase)
    ) && !(mode.kind == ModeKind::Normal && cake_choice(&game.id, &mode.key).is_some())
    {
        return None;
    }
    let row = GAMES
        .get_or_init(|| {
            serde_json::from_slice(include_bytes!("../../bindings/habanero.json"))
                .expect("valid embedded Habanero bindings")
        })
        .get(&game.id)?;
    Some((row, row.modes.get(&mode.key)?.as_str()))
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    binding(game, mode).is_some()
}

pub(crate) fn missing_binding_detail(game: &Game, mode: &Mode) -> Option<&'static str> {
    if mode.kind == ModeKind::Booster
        && mode.key == "superbet"
        && matches!(game.id.as_str(), "SGLaughingBuddha" | "SGZeusDeluxe")
    {
        return Some(
            "Habanero public client does not send the Super Bet selector for this game; the historical superBet=1 request was charged as a base spin",
        );
    }
    None
}

fn amount(value: &Value) -> Result<Decimal> {
    match value {
        Value::String(s) => decimal(s),
        Value::Number(n) => decimal(&n.to_string()),
        _ => bail!("Habanero amount missing"),
    }
}

fn text<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("Habanero field {name} missing"))
}

fn stake(init: &Value) -> Result<Decimal> {
    if let Some(menu) = init["stakeincrement"].as_str().filter(|s| !s.is_empty()) {
        return menu
            .split('|')
            .filter(|s| !s.is_empty())
            .map(decimal)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|d| *d > Decimal::ZERO)
            .min()
            .context("Habanero minimum stake missing");
    }
    let default = amount(&init["defaultstake"])?;
    ensure!(
        default > Decimal::ZERO,
        "Habanero default stake is nonpositive"
    );
    Ok(default)
}

async fn send(ctx: &RunContext, label: &str, body: &Value, financial: bool) -> Result<Value> {
    if financial {
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
    let reply = ctx
        .json(
            label,
            API,
            body,
            BTreeMap::from([
                ("origin".into(), "https://app-test.insvr.com".into()),
                ("referer".into(), "https://app-test.insvr.com/".into()),
                ("accept".into(), "*/*".into()),
            ]),
            financial,
        )
        .await?;
    let error = &reply["error"]["code"];
    ensure!(
        error.is_null() || error == 0 || error == "",
        "Habanero native command rejected"
    );
    ensure!(
        reply["header"]["player"]["funmode"] != false,
        "Habanero response contradicted demo mode"
    );
    Ok(reply)
}

fn request(header: &Value, game: &Value, port: Option<&Value>) -> Value {
    let mut body = json!({"header":header,"game":game,"grid":Uuid::new_v4().to_string()});
    if let Some(port) = port {
        body["portmessage"] = json!(port.to_string());
    }
    body
}

fn validate_owner(reply: &Value, game: &Game, row: &Binding, sid: &str) -> Result<()> {
    let native = &reply["game"];
    ensure!(
        native.is_object()
            && native["sessionid"].as_str().is_none_or(|v| v == sid)
            && native["kn"].as_str().is_none_or(|v| v == game.id)
            && native["brandgameid"]
                .as_str()
                .is_none_or(|v| v == row.brand),
        "Habanero round owner or game changed"
    );
    Ok(())
}

fn continuation(game: &str, message: &Value, pssid: &str) -> Result<Value> {
    let mut next = json!({"gameMode":"free","pssid":pssid,"gssid":text(message, "gssid").map_err(|_|unknown_event("Habanero open continuation has no gssid"))?});
    // The handoff PROTOCOL.md plus completed replay/pick passes prove these native
    // controls. Case varies by client revision; unknown names never fall through.
    let state = message["nextgamestate"]
        .as_str()
        .ok_or_else(|| unknown_event("Habanero next state missing"))?
        .to_ascii_lowercase();
    match state.as_str() {
        "freegame_progress" if game == "SGSteampunkPlinko" => {
            next["fgProgress"] = json!({"isComplete":true})
        }
        "freegame" | "main" | "maincascade" | "freegamecascade" => {}
        "respin"
            if matches!(
                game,
                "SGChristmasGiftRush" | "SGSuperFruitBlast" | "SGBattleTheBeast"
            ) => {}
        "freerespin" if game == "SGRacingRoyalty" => {}
        "freegame_locked_win" if game == "SGShamrockQuest" => {}
        "superfreegame" if game == "SGNuwaDeluxe" => {}
        "freegamegamble" if game == "SGGladiatorRoyal" => {}
        "pick" if matches!(game, "SGGladiatorRoyal" | "SGMXMania" | "SGSteampunkPlinko") => {
            next["gameMode"] = json!("pick");
            next["pickIndex"] = json!(0);
        }
        "symbolpick" if game == "SGLaughingBuddha" => {
            next["gameMode"] = json!("pick");
            next["pickIdIndex"] = json!(0);
        }
        _ => {
            return Err(unknown_event(format!(
                "Habanero unsupported native state for {game}: {state}"
            )));
        }
    }
    Ok(next)
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &serde_json::Value,
) -> Result<Completion> {
    let (row, _factor) =
        binding(game, mode).context("Habanero mode lacks verified price and launch parameters")?;
    if let Some(source) = row.sources.get(&mode.key) {
        let asset = url::Url::parse(&source.url)?;
        ensure!(
            asset.path().starts_with(&format!(
                "/gamecontent/{}-UNIVERSAL_v",
                source_game(&game.id)
            )),
            "Habanero public pricing source belongs to another game"
        );
        let reply = ctx.get("public_price_source", asset.as_str()).await?;
        ensure!(reply.status == 200, "public client source HTTP error");
        if !(sha256(reply.body.as_bytes()) == source.sha256) {
            return Err(unknown_event("Habanero public price source changed"));
        }
    }
    let mut header = json!({
        "player":{"sfb":null,"dtld":639295006360530300_u64,"ssotoken":"","funmode":true,
                  "brandid":"a9d542ac-c9bb-e311-93f6-80c16e0883f4","lc":"en","mobile":false},
        "version":"1.1.24603.707"
    });
    let mut native = json!({"action":"init","sessionid":null,"brandgameid":row.brand,"kn":game.id});
    let opened = send(ctx, "init", &request(&header, &native, None), false).await?;
    let sid = text(&opened["game"], "sessionid")?;
    let token = text(&opened["header"]["player"], "ssotoken")?;
    validate_owner(&opened, game, row, sid)?;

    let coin = stake(&opened["game"]["init"])?;
    let coin_json: Value = serde_json::from_str(&coin.to_string())?;
    native["sessionid"] = json!(sid);
    header["player"]["ssotoken"] = json!(token);
    ctx.save_session(&json!({"game":game.id,"sessionid":sid,"header":header,"service":API}))?;
    let (steps, round) = if row.legacy {
        native["action"] = json!("spin");
        native["play"] = json!({"betstate":{"bet":[{"betlevel":1,"stake":coin_json,"numpaylines":row.lines,"coins":row.coins}]}});
        let reply = send(ctx, "spin", &request(&header, &native, None), true).await?;
        validate_owner(&reply, game, row, sid)?;
        let play = &reply["game"]["play"];
        if play["videoslotstate"]["gamemodename"] != "main" {
            return Err(unknown_event(
                "Habanero legacy round requires an unimplemented continuation",
            ));
        }

        (1, reply["game"]["gameid"].as_str().map(str::to_owned))
    } else {
        native["action"] = json!("game");
        let mut pssid = opened["portmessage"]["pssid"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let mut port = json!({"coinValue":coin_json,"betLevel":1,"numLines":row.lines,"gameMode":"base","pssid":pssid});
        match mode.key.as_str() {
            "buy" => port["featureBuy"] = json!(1),
            "superbet" => port["superBet"] = json!(1),
            _ => {}
        }
        if let Some(choice) = cake_choice(&game.id, &mode.key) {
            port["CakeValley_selectedMode"] = json!(choice);
        }
        if game.id == "SGJellyfishFlowUltra" {
            port["betMode"] = json!("ultra");
        }
        let mut steps = 1;
        let mut reply = send(ctx, "entry", &request(&header, &native, Some(&port)), true).await?;

        let mut root = None;
        loop {
            validate_owner(&reply, game, row, sid)?;
            let message = &reply["portmessage"];
            let done = message["isgamedone"]
                .as_bool()
                .ok_or_else(|| unknown_event("Habanero native closure flag missing"))?;
            if let Some(id) = reply["game"]["gameid"].as_str().filter(|s| !s.is_empty())
                && root.is_none()
            {
                root = Some(id.to_owned());
            }
            if done {
                break (steps, root);
            }
            ensure!(
                steps < ctx.max_steps(),
                "Habanero continuation limit reached"
            );
            if let Some(next) = message["pssid"].as_str().filter(|s| !s.is_empty()) {
                pssid = next.into();
            }
            port = continuation(&game.id, message, &pssid)?;
            // Native gameid is the previous response identifier, as the handoff
            // protocol documents; gssid and pssid separately carry the bonus state.
            native["gameid"] = json!(
                text(&reply["game"], "gameid")
                    .map_err(|_| unknown_event("Habanero open response gameid missing"))?
            );
            reply = send(
                ctx,
                "continuation",
                &request(&header, &native, Some(&port)),
                true,
            )
            .await?;
            steps += 1;
        }
    };
    Ok(Completion {
        native_round_id: round,
        details: json!({"steps":steps,"mode":mode.key}),
    })
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn native_freegame_ack_without_money() {
        let value = continuation(
            "SGSteampunkPlinko",
            &json!({"nextgamestate":"freegame_progress","gssid":"r"}),
            "s",
        )
        .unwrap();
        assert_eq!(value["fgProgress"]["isComplete"], true);
        assert_eq!(value["gssid"], "r");
    }
    #[test]
    fn unknown_or_unbound_respin_alerts() {
        for state in ["new_state", "respin"] {
            assert!(
                continuation(
                    "SGCakeValley",
                    &json!({"nextgamestate":state,"gssid":"r"}),
                    "s"
                )
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
            );
        }
    }
}

#[cfg(test)]
mod handoff_tests {
    use super::*;
    #[test]
    fn supplied_handoff_pick_controls_preserve_previous_state_ids() {
        for (game, state, key) in [
            ("SGGladiatorRoyal", "pick", "pickIndex"),
            ("SGMXMania", "pick", "pickIndex"),
            ("SGSteampunkPlinko", "PICK", "pickIndex"),
            ("SGLaughingBuddha", "symbolpick", "pickIdIndex"),
        ] {
            let request = continuation(
                game,
                &json!({"nextgamestate":state,"gssid":"native-round"}),
                "latest-state",
            )
            .unwrap();
            assert_eq!(request[key], 0);
            assert_eq!(request["gameMode"], "pick");
            assert_eq!(request["gssid"], "native-round");
            assert_eq!(request["pssid"], "latest-state");
        }
    }
    #[test]
    fn supplied_handoff_automatic_native_states_are_recognized_without_wallet() {
        for (game, state) in [
            ("SGHyperHues", "freegamecascade"),
            ("SGShamrockQuest", "freegame_locked_win"),
            ("SGRacingRoyalty", "FREERESPIN"),
            ("SGNuwaDeluxe", "SUPERFREEGAME"),
            ("SGGladiatorRoyal", "FREEGAMEGAMBLE"),
            ("SGSuperFruitBlast", "respin"),
        ] {
            assert_eq!(
                continuation(game, &json!({"nextgamestate":state,"gssid":"r"}), "s").unwrap()["gameMode"],
                "free"
            );
        }
    }
    #[test]
    fn state_controls_do_not_leak_to_unbound_games() {
        assert!(
            continuation(
                "SGCakeValley",
                &json!({"nextgamestate":"PICK","gssid":"r"}),
                "s"
            )
            .unwrap_err()
            .downcast_ref::<crate::model::UnknownEvent>()
            .is_some()
        );
    }
}
