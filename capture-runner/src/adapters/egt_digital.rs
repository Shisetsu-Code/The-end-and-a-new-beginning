//! Native capture connector derived from standalone info/egt-digital/codigo/egt_digital.rs.
//! Published source SHA-256: 6620b8647c2ec324244bea936538269e04963c67c67ce360da73ac76c963cde2
//! Reference transport: game-runner/src/adapters/egt_digital.rs; no runtime dependency.
//! EGT Digital native demo SockJS sessions.
use crate::{
    context::{DecodedFrame, NativeSocket, RunContext, SocketPayload},
    model::{Completion, Game, Mode},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use url::Url;
use uuid::Uuid;

const ORIGIN: &str = "https://games.egt-ong.com";
const VERSION: &str = "1.64.4";
const BROWSER: &str = "Firefox 159 Windows";

fn bindings() -> &'static Value {
    static MAP: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        serde_json::from_str(include_str!("../../bindings/egt-digital.json"))
            .expect("embedded EGT Digital variant bindings")
    })
}

fn binding(game: &Game, mode: &Mode) -> Option<&'static Value> {
    let bound = bindings().get(&game.id)?.get(&mode.key)?;
    (bound["kind"] == mode.kind.as_str()).then_some(bound)
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    binding(game, mode).is_some()
}

fn number(data: &Value, key: &str) -> Result<u64> {
    data[key]
        .as_u64()
        .with_context(|| format!("EGT Digital missing integer {key}"))
}

fn offered_index(values: &Value, indices: &Value, wanted: &Value) -> Result<bool> {
    let values = values
        .as_array()
        .context("EGT Digital stake options missing")?;
    let indices = indices
        .as_array()
        .context("EGT Digital stake indices missing")?;
    if indices.is_empty() {
        return Ok(values.contains(wanted));
    }
    let mut offered = false;
    for index in indices {
        let index = index
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .context("EGT Digital malformed stake index")?;
        offered |= values
            .get(index)
            .context("EGT Digital stake index out of range")?
            == wanted;
    }
    Ok(offered)
}

fn verified_cost(settings: &Value, bet: &Value) -> Result<u64> {
    let offers = settings["gameModes"]
        .as_array()
        .context("EGT Digital mode offers missing")?;
    let mut matching = offers.iter().filter(|offer| {
        offer["gameMode"] == bet["gameMode"]
            && offer["factor"] == bet["factor"]
            && (offer["lines"] == bet["lines"] || bet["lines"] == 0)
    });
    let offer = matching.next().context("EGT Digital mode offer changed")?;
    ensure!(
        matching.next().is_none(),
        "EGT Digital ambiguous native mode offer"
    );
    ensure!(
        offered_index(&settings["bets"], &offer["betIndices"], &bet["level"])?
            && offered_index(
                &settings["denominations"],
                &offer["denominationIndices"],
                &bet["denomination"]
            )?,
        "EGT Digital native stake option changed"
    );
    number(bet, "factor")?
        .checked_mul(number(bet, "level")?)
        .and_then(|base| base.checked_mul(bet["denomination"].as_u64()?))
        .filter(|cost| *cost > 0)
        .context("EGT Digital invalid entry cost")
}

fn sockjs_frame(frame: &SocketPayload, sequence: u64) -> Result<DecodedFrame> {
    let SocketPayload::Text(text) = frame else {
        return Err(crate::model::unknown_event(
            "EGT Digital unexpected binary frame".to_string(),
        ));
    };
    let mut decoded = DecodedFrame {
        values: Vec::new(),
        replies: Vec::new(),
    };
    if text == "h" {
        return Ok(decoded);
    }
    let data = text
        .strip_prefix("a[")
        .ok_or_else(|| crate::model::unknown_event("EGT Digital unexpected SockJS frame"))?;
    for message in serde_json::from_str::<Vec<String>>(&format!("[{data}"))? {
        let value: Value = serde_json::from_str(&message)?;
        if value["event"] == "echo" {
            decoded
                .replies
                .push(SocketPayload::Text(serde_json::to_string(&[message])?));
        } else if value["referenceId"] == sequence {
            decoded.values.push(value);
        }
    }
    Ok(decoded)
}

async fn command(
    ctx: &RunContext,
    socket: &mut NativeSocket,
    seq: &mut u64,
    mut payload: Value,
    round_action: bool,
) -> Result<Value> {
    *seq += 1;
    let id = *seq;
    payload["id"] = json!(id);
    let label = payload["event"].as_str().context("missing native event")?;
    let data = serde_json::to_string(&[serde_json::to_string(&payload)?])?;
    let response = ctx
        .ws_codec_controlled(
            socket,
            label,
            SocketPayload::Text(data),
            round_action,
            None,
            move |frame| sockjs_frame(frame, id),
        )
        .await?;
    ensure!(
        response["event"] == label && response["error"].is_null(),
        "EGT Digital native event rejected"
    );
    Ok(response)
}

fn continuation(state: &str, bet: &Value) -> Result<Value> {
    match state {
        "freespin" | "holdspin" | "respin" | "freeSpinStickyWilds" | "freeSpinRainingWilds" => {
            Ok(json!({"event":"bet","bet":bet,"context":{}}))
        }
        "win" => Ok(
            json!({"event":"collect","bet":{"level":bet["level"],"denomination":bet["denomination"],
            "lines":bet["lines"],"factor":bet["lines"]}}),
        ),
        _ => Err(crate::model::unknown_event(
            "EGT Digital continuation requires an unimplemented native control".to_string(),
        )),
    }
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    let bound = binding(game, mode).context("missing captured EGT Digital mode binding")?;
    let bet = &bound["bet"];
    let token = Uuid::new_v4().to_string();
    let socket_id = Uuid::new_v4().simple().to_string();
    let mut url = Url::parse(&format!(
        "wss://game-server-demo.egt-ong.com/game-websocket/123/{}/websocket",
        &socket_id[..16]
    ))?;
    url.query_pairs_mut().extend_pairs([
        ("demo", "true"),
        ("sessionToken", &token),
        ("playerId", "1101"),
        ("gameKey", &game.id),
        ("version", VERSION),
        ("browser", BROWSER),
        ("lang", "en"),
    ]);
    let mut socket = ctx
        .websocket(url.as_str(), ORIGIN, None, BTreeMap::new())
        .await?;
    ctx.ws_expect_text(&mut socket, "sockjs_open", "o").await?;
    let mut seq = 0;
    let init = command(
        ctx,
        &mut socket,
        &mut seq,
        json!({"event":"loadGame","gameKey":game.id,
        "context":{"clientVersion":VERSION,"browser":BROWSER}}),
        false,
    )
    .await?;
    let owner = init["sessionKey"]
        .as_str()
        .filter(|key| !key.is_empty())
        .context("EGT Digital owner missing")?;
    ensure!(
        init["state"] == "idle" && init["gameKey"] == game.id,
        "EGT Digital did not confirm an idle demo session"
    );
    let cost = verified_cost(&init["settings"], bet)?;
    ctx.save_session(
        &json!({"provider":"egt-digital","game":game.id,"sessionKey":owner,"url":url.as_str(),
        "currency":"EGT","mode":mode.key,"bet":bet,"cost":cost,"source":bound["evidence_run"]}),
    )?;
    let mut payload = json!({"event":"bet","bet":bet,"context":{}});
    let mut match_id = None;
    for step in 0..ctx.max_steps().min(300) {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let answer = command(ctx, &mut socket, &mut seq, payload, true).await?;
        ensure!(
            answer["sessionKey"] == owner,
            "EGT Digital demo owner changed"
        );
        let state = answer["state"]
            .as_str()
            .ok_or_else(|| crate::model::unknown_event("EGT Digital round state missing"))?;
        match answer
            .pointer("/game/state/matchId")
            .and_then(Value::as_str)
        {
            Some(received) if !received.is_empty() => {
                ensure!(
                    match_id.as_deref().is_none_or(|known| known == received),
                    "EGT Digital native match changed"
                );
                match_id.get_or_insert_with(|| received.to_owned());
            }
            _ => {}
        }
        if state != "idle" {
            payload = continuation(state, bet)?;
            continue;
        }
        return Ok(Completion {
            native_round_id: match_id,
            details: json!({"bet":bet,"steps":step+1,"source":bound["evidence_run"]}),
        });
    }
    Err(crate::model::unknown_event(
        "EGT Digital round exceeded the continuation limit",
    ))
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn bet_offer_needs_no_balance() {
        let bet = json!({"gameMode":"normal","factor":20,"lines":20,"level":1,"denomination":1});
        let settings = json!({"gameModes":[{"gameMode":"normal","factor":20,"lines":20,"betIndices":[],"denominationIndices":[]}],"bets":[1],"denominations":[1]});
        assert_eq!(verified_cost(&settings, &bet).unwrap(), 20);
        assert!(
            continuation("new_feature", &bet)
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
    #[test]
    fn sockjs_preserves_native_terminal_without_balance() {
        let raw = json!({"referenceId":2,"event":"bet","state":"idle"}).to_string();
        let frame = SocketPayload::Text(format!("a{}", json!([raw])));
        assert_eq!(sockjs_frame(&frame, 2).unwrap().values[0]["state"], "idle");
    }
}
