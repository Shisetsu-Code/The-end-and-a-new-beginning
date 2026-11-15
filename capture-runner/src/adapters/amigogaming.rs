//! Native capture connector derived from standalone info/amigogaming/codigo/amigogaming.rs.
//! Published source SHA-256: d44888bdc8c0e2c2183bb446ece1d5005c73d47694d25beb83a69ee5b0482f8b
//! Reference transport: game-runner/src/adapters/amigogaming.rs; no runtime dependency.
use crate::{
    context::{NativeSocket, RunContext, SocketPayload},
    model::{Completion, Game, Mode, ModeKind, decimal, exact_mul},
};
use anyhow::{Context, Result, bail, ensure};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

const PORTAL: &str = "https://amigogaming.com";
const DEMO: &str = "https://demo.amigogaming.cloud";
const SPIN: &str = "features.slot.action.SpinAction";

pub fn supports(game: &Game, mode: &Mode) -> bool {
    game.id
        .parse::<u64>()
        .is_ok_and(|id| id > 0 && id.to_string() == game.id)
        && match mode.kind {
            ModeKind::Normal => mode.key == "NORMAL",
            ModeKind::Purchase => mode.key.strip_prefix("BUY_").is_some_and(|id| {
                !id.is_empty()
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            }),
            _ => false,
        }
}

fn array<'a>(value: &'a Value, name: &str) -> Result<&'a [Value]> {
    value[name]
        .as_array()
        .map(Vec::as_slice)
        .with_context(|| format!("Amigo menu {name} absent"))
}

fn text<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("Amigo field {name} absent"))
}

fn money(value: &Value) -> Result<Decimal> {
    match value {
        Value::String(s) => decimal(s),
        Value::Number(n) => decimal(&n.to_string()),
        _ => bail!("Amigo amount absent"),
    }
}

async fn post_form(
    ctx: &RunContext,
    label: &str,
    url: &str,
    fields: &[(&str, &str)],
    csrf: Option<&str>,
) -> Result<String> {
    let form: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish();
    let mut headers = BTreeMap::from([(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    )]);
    if let Some(token) = csrf {
        headers.insert("x-csrf-token".into(), token.into());
    }
    let reply = ctx.http(label, "POST", url, headers, form, false).await?;
    ensure!(reply.status == 200, "Amigo {label} HTTP {}", reply.status);
    Ok(reply.body)
}

async fn launch(ctx: &RunContext, game: &Game) -> Result<(Value, String, Url)> {
    let portal = ctx
        .get("demo_portal", &format!("{PORTAL}/es/games"))
        .await?;
    ensure!(portal.status == 200, "Amigo portal unavailable");
    let csrf = portal
        .body
        .split_once("<meta name=\"csrf-token\" content=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(token, _)| token);
    let entry = post_form(
        ctx,
        "demo_issuer",
        &format!("{PORTAL}/es/generateDemoGameLink"),
        &[("id", &game.id), ("sessionType", "desktop")],
        csrf,
    )
    .await?;
    let mut url = Url::parse(entry.trim()).context("Amigo demo link absent")?;
    ensure!(
        url.host_str() == Some("launch.amigogaming.com"),
        "unexpected Amigo issuer target"
    );
    let mut reached = false;
    for _ in 0..12 {
        let reply = ctx.get("demo_redirect", url.as_str()).await?;
        if reply.status == 200 {
            reached = url.host_str() == Some("demo.amigogaming.cloud")
                && url.path() == "/js-frontend/html5/index.html";
            break;
        }
        ensure!(
            matches!(reply.status, 301 | 302 | 303 | 307 | 308),
            "Amigo unexpected launch response"
        );
        url = url.join(
            reply
                .headers
                .get("location")
                .context("Amigo redirect without location")?,
        )?;
    }
    ensure!(reached, "Amigo demo frontend not reached");
    let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    let endpoint = Url::parse(query.get("launch").context("Amigo native launch absent")?)?;
    if !(endpoint.scheme() == "https"
        && endpoint.host_str() == Some("demo.amigogaming.cloud")
        && endpoint.path() == "/organic2/websocket/launch"
        && endpoint.query().is_none())
    {
        return Err(crate::model::unknown_event(
            "unreviewed Amigo native launch".to_string(),
        ));
    };
    let session = query
        .get("session")
        .context("Amigo launch session absent")?;
    let sign = query.get("sign").context("Amigo launch signature absent")?;
    let body = post_form(
        ctx,
        "native_launch",
        endpoint.as_str(),
        &[
            ("session", session),
            ("sign", sign),
            ("info", "Evidencia public demo QA; Windows"),
            (
                "mobile",
                query.get("mobile").map_or("false", String::as_str),
            ),
        ],
        csrf,
    )
    .await?;
    let native: Value = serde_json::from_str(&body)?;
    let resources = Url::parse(text(&native, "resourcesLocation")?)?;
    if !(resources.host_str() == Some("demo.amigogaming.cloud")
        && resources.path().starts_with("/games-alt-wgl/amigo/")
        && resources.path().ends_with("/index.html"))
    {
        return Err(crate::model::unknown_event(
            "unreviewed Amigo resources".to_string(),
        ));
    };
    let engine_game = text(&native, "game")?
        .strip_prefix("amigo_")
        .context("Amigo game identity absent")?;
    let normalize = |s: &str| {
        s.bytes()
            .filter(u8::is_ascii_alphanumeric)
            .map(|b| b.to_ascii_lowercase())
            .collect::<Vec<_>>()
    };
    ensure!(
        normalize(text(&native, "gameName")?) == normalize(&game.name)
            || normalize(engine_game) == normalize(&game.name),
        "Amigo launch game differs from inventory"
    );
    ensure!(
        resources.path().split('/').nth_back(1) == Some(engine_game),
        "Amigo resources game differs from launch"
    );
    let params: BTreeMap<_, _> = resources.query_pairs().into_owned().collect();
    let mut socket = Url::parse(text(&native, "endpoint")?)?;
    ensure!(
        socket.query().is_none(),
        "Amigo socket includes unexplained parameters"
    );
    let ws_session = text(&native, "session")?;
    let ws_sign = text(&native, "sign")?;
    ensure!(
        params.get("session").map(String::as_str) == Some(ws_session)
            && params.get("sign").map(String::as_str) == Some(ws_sign),
        "Amigo socket owner differs from resources"
    );
    socket
        .query_pairs_mut()
        .append_pair("session", ws_session)
        .append_pair("sign", ws_sign);

    let document = ctx.get("demo_resources", resources.as_str()).await?;
    ensure!(document.status == 200, "Amigo game document unavailable");
    let marker = regex::Regex::new(r#"var gameLib\s*=\s*"([^"]+)""#)?;
    let path = marker
        .captures(&document.body)
        .and_then(|m| m.get(1))
        .context("Amigo client not declared")?;
    let script = ctx
        .get("demo_client", resources.join(path.as_str())?.as_str())
        .await?;
    ensure!(
        script.status == 200
            && [
                "requests.InitRequest",
                "requests.ActionRequest",
                SPIN,
                "JSON.stringify"
            ]
            .iter()
            .all(|marker| script.body.contains(marker)),
        "Amigo client transport changed"
    );
    Ok((native, script.body, socket))
}

fn min_bet(init: &Value) -> Result<(Value, Decimal)> {
    let minimum = init
        .get("minBet")
        .map(money)
        .transpose()?
        .unwrap_or(Decimal::ZERO);
    let maximum = money(&init["maxBet"])?;
    let mut best = None;
    for bet in array(init, "bets")? {
        for multiplier in array(init, "multipliers")? {
            for denom in array(init, "denominations")? {
                let value = exact_mul(
                    exact_mul(money(&bet["value"])?, money(multiplier)?)?,
                    money(denom)?,
                )?;
                let key = (
                    value,
                    money(&bet["value"])?,
                    money(multiplier)?,
                    money(denom)?,
                );
                if value > Decimal::ZERO
                    && value >= minimum
                    && value <= maximum
                    && best.as_ref().is_none_or(|(previous, _)| key < *previous)
                {
                    best = Some((
                        key,
                        json!({"@type":"BetParams","bet":bet["value"],"multiplier":multiplier,"denomination":denom}),
                    ));
                }
            }
        }
    }
    best.map(|(key, params)| (params, key.0))
        .context("no legal Amigo minimum bet")
}

fn active_feature(reply: &Value) -> Result<&str> {
    let states = &reply["states"];
    if let Some(id) = states["initialized"]
        .as_str()
        .filter(|id| states[*id]["status"] == "INIT")
    {
        return Ok(id);
    }
    let mut id = text(states, "current")?;
    let mut visited = BTreeSet::new();
    while id != "SLOT" && states[id]["status"] == "FINISHED" {
        ensure!(visited.insert(id), "cyclic Amigo feature state");
        id = states[id]["parentFeatureId"].as_str().unwrap_or("SLOT");
    }
    Ok(id)
}

fn closed(reply: &Value, feature: &str) -> bool {
    feature == "SLOT"
        && reply["states"]["SLOT"]["status"] == "FINISHED"
        && reply["states"]["SLOT"]["stepsAvailable"]
            .as_u64()
            .unwrap_or(0)
            == 0
}

fn continuation(
    reply: &Value,
    init: &Value,
    feature: &str,
    client: &str,
    bet: &Value,
) -> Result<Value> {
    match text(&init["features"][feature], "@type")? {
        "features.slot.SlotFeature" => Ok(json!({"@type":SPIN,"betParams":bet})),
        "features.pickme.PickMeFeature" | "features.match3.Match3Feature" => {
            let kind = text(&init["features"][feature], "@type")?;
            let marker = if kind == "features.match3.Match3Feature" {
                client.contains(".EventGame.Match3Data)")
            } else {
                client.contains(".EventGame.PickMeData)")
                    || client.contains(".EventGame.SurpriseResult)")
            };
            ensure!(
                marker
                    && client.contains(".RequestAction.PickMe,null,")
                    && client.contains(".action.choice="),
                "Amigo choice binding not reviewed"
            );
            let state = &reply["states"][feature];
            if kind == "features.pickme.PickMeFeature" {
                ensure!(
                    state["steps"].as_u64().is_some_and(|value| value > 0),
                    "no Amigo choices available"
                );
            }
            let choices = array(state, "choices")?;
            let index = choices
                .iter()
                .position(|value| value.as_i64() == Some(-1))
                .context("no hidden Amigo choice")?;
            Ok(json!({"@type":"features.pickme.action.PickMeAction","choice":index}))
        }
        _ => Err(crate::model::unknown_event(
            "unreviewed Amigo continuation feature".to_string(),
        )),
    }
}

async fn send(
    ctx: &RunContext,
    socket: &mut NativeSocket,
    label: &str,
    request: Value,
    id: usize,
    round_action: bool,
) -> Result<Value> {
    let payload = SocketPayload::Text(serde_json::to_string(&request)?);
    ctx.ws_codec(
        socket,
        label,
        payload,
        round_action,
        Some(&id.to_string()),
        |frame| {
            let SocketPayload::Text(body) = frame else {
                bail!("unexpected Amigo binary message");
            };
            let response: Value = serde_json::from_str(body)?;
            ensure!(
                response["@type"] != "responses.ErrorResponse",
                "Amigo native request rejected"
            );
            Ok(vec![response])
        },
    )
    .await
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    let (native, client, uri) = launch(ctx, game).await?;
    ctx.save_session(&json!({"game":game.id,"launch":native,"endpoint":uri.as_str()}))?;
    let mut socket = ctx
        .websocket(uri.as_str(), DEMO, None, BTreeMap::new())
        .await?;
    let init = send(
        ctx,
        &mut socket,
        "init",
        json!({"@type":"requests.InitRequest","id":0}),
        0,
        false,
    )
    .await?;
    ensure!(
        init["@type"] == "responses.InitResponse"
            && init["round"] == 0
            && init["mainFeature"] == "SLOT"
            && closed(&init, active_feature(&init)?),
        "Amigo demo did not initialize an idle slot"
    );
    let (bet, _) = min_bet(&init)?;
    let mut action = json!({"@type":SPIN,"betParams":bet});
    if let Some(id) = mode.key.strip_prefix("BUY_") {
        ensure!(
            client.contains(".action.buyInId=this.buyInId"),
            "Amigo purchase control absent"
        );
        ensure!(
            array(&init, "buyIns")?
                .iter()
                .any(|offer| offer["id"] == id),
            "Amigo purchase not offered"
        );
        action["buyInId"] = json!(id);
    }
    let mut root = None;
    for id in 1..=ctx.max_steps() {
        let response = send(
            ctx,
            &mut socket,
            "action",
            json!({"@type":"requests.ActionRequest","id":id,"action":action}),
            id,
            true,
        )
        .await?;
        ensure!(
            response["@type"] == "responses.ActionResponse",
            "Amigo unexpected action response type"
        );
        if let Some(round) = response["round"].as_u64().filter(|n| *n > 0) {
            ensure!(
                root.is_none_or(|prior| prior == round),
                "Amigo native round identity changed"
            );
            root = Some(round);
        }
        let feature = active_feature(&response)?;
        if closed(&response, feature) {
            return Ok(Completion {
                native_round_id: root.map(|round| round.to_string()),
                details: json!({"steps":id,"mode":mode.key}),
            });
        }
        action = continuation(&response, &init, feature, &client, &bet)?;
    }
    Err(crate::model::unknown_event(
        "Amigo continuation limit reached",
    ))
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn native_closure_and_unknown_feature_without_balance() {
        let reply =
            json!({"states":{"current":"SLOT","SLOT":{"status":"FINISHED","stepsAvailable":0}}});
        assert!(closed(&reply, active_feature(&reply).unwrap()));
        let init = json!({"features":{"NEW":{"@type":"features.NewGame"}}});
        assert!(
            continuation(&Value::Null, &init, "NEW", "", &json!({}))
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
