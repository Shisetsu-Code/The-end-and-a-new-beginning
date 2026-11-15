//! Native protocol reconstructed from standalone info/redtiger/codigo; no old runtime dependency.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, unknown_event},
};
use anyhow::{Context, Result, ensure};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::Url;
use uuid::Uuid;

const FANSITE: &str = "https://fansite.evo-games.com";
const VERSION: &str = "6.20261113.73039.67536-2dc7fa4ff6-r2";
const CLIENT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) hardfire/1.5.1 Chrome/152.0.7977.130 Electron/44.5.1 Safari/537.36";
// Public frontend identifier published in Red Tiger's demo JS.
const CLIENT: &str = "ATp3ZRaTE2u22oTM6GXmHtkgWYc6F8vZ";

pub(super) fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("missing Red Tiger field {key}"))
}

pub(super) fn money(v: &Value) -> Result<Decimal> {
    if let Some(s) = v.as_str() {
        return decimal(s);
    }
    ensure!(v.is_number(), "missing native amount");
    decimal(&v.to_string())
}

fn reviewed_choice(game: &Game, mode: &Mode) -> Option<&'static str> {
    if game.id != "aztectribute0000" || mode.kind != ModeKind::Purchase {
        return None;
    }
    match mode.key.as_str() {
        "101:WildMultiplier" => Some("WildMultiplier"),
        "101:WaysMultiplier" => Some("WaysMultiplier"),
        "101:Random" => Some("Random"),
        _ => None,
    }
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    if reviewed_choice(game, mode).is_some() {
        return true;
    }
    if game.id == "aztectribute0000" && mode.key == "101" {
        return false;
    }
    match (mode.kind, mode.key.parse::<u64>()) {
        (ModeKind::Normal, Ok(0)) => mode.key == "0",
        (ModeKind::Purchase | ModeKind::AnteBet | ModeKind::Variant, Ok(id)) => {
            id > 0 && id.to_string() == mode.key
        }
        _ => false,
    }
}

fn available_choices(game: &Value) -> Result<Option<&Vec<Value>>> {
    let choices = &game["choices"];
    let available = match choices {
        Value::Object(fields) if fields.is_empty() => return Ok(None),
        Value::Object(fields) => fields
            .get("available")
            .ok_or_else(|| unknown_event("Red Tiger unrecognized choice menu"))?,
        _ => choices,
    };
    match available {
        Value::Null => Ok(None),
        Value::Array(options) => Ok((!options.is_empty()).then_some(options)),
        _ => Err(unknown_event("Red Tiger unrecognized choice menu")),
    }
}

fn validate_math_mode(game: &Value, id: u64, kind: &ModeKind) -> Result<()> {
    let expected_type = match (kind, id) {
        (ModeKind::Normal, 0) => return Ok(()),
        (ModeKind::Purchase, 1..) => "featureBuy",
        (ModeKind::AnteBet, 1..) => "chanceIncrease",
        (ModeKind::Variant, 1..) => "stateSwitch",
        _ => anyhow::bail!("unreviewed Red Tiger modality"),
    };
    let mut offers = game["mathModes"]
        .as_array()
        .context("native math mode menu absent")?
        .iter()
        .filter(|offer| offer["id"].as_u64() == Some(id));
    let offer = offers.next().context("native math mode not offered")?;
    ensure!(
        offers.next().is_none() && offer["type"] == expected_type,
        "native math mode is ambiguous or changed type"
    );
    Ok(())
}

fn finish_ack(finish: &Value, round: &Value, previous: &Value) -> Result<bool> {
    let closed = &finish["result"];
    ensure!(
        finish["success"] == true && closed["transactions"]["roundId"] == *round,
        "native finish did not acknowledge this round"
    );
    same_user(closed, previous)?;
    if closed.get("game").is_some() {
        ensure!(
            closed["game"]["hasState"] == false,
            "native finish did not confirm closure"
        );
        return Ok(false);
    }
    ensure!(
        text(&closed["user"], "sessionId")? == text(&previous["user"], "sessionId")?,
        "Red Tiger finish session changed"
    );
    Ok(true)
}

pub(super) fn same_user(next: &Value, previous: &Value) -> Result<()> {
    let expected = &previous["user"]["userId"];
    ensure!(
        (expected.is_number() || expected.as_str().is_some_and(|id| !id.is_empty()))
            && next["user"]["userId"] == *expected,
        "Red Tiger response changed demo user"
    );
    Ok(())
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    ensure!(supports(game, mode), "unsupported Red Tiger game mode");
    let table = &game.id;
    ensure!(
        !table.is_empty() && table.bytes().all(|b| b.is_ascii_alphanumeric()),
        "invalid table id"
    );
    let launch = format!("https://redtiger.com/demo/{table}?showNavbar=true");
    ensure!(
        ctx.get("demo_page", &launch).await?.status == 200,
        "demo page unavailable"
    );
    let issued = ctx
        .json(
            "demo_issuer",
            "https://jinx.prod.cmsevo.com/api/v1/oss/token/demo",
            &json!({"demo":{"language":"en-GB","currency":"VC0"},"game":{"tableId":table}}),
            BTreeMap::from([
                ("origin".into(), "https://redtiger.com".into()),
                ("referer".into(), "https://redtiger.com/".into()),
                ("x-api-key".into(), CLIENT.into()),
                ("accept".into(), "*/*".into()),
                ("accept-language".into(), "es".into()),
                ("user-agent".into(), CLIENT_UA.into()),
            ]),
            false,
        )
        .await?;
    let mut url = Url::parse(FANSITE)?.join(text(&issued, "entryEmbedded")?)?;
    let mut reached = false;
    for hop in 0..4 {
        ensure!(
            url.origin().ascii_serialization() == FANSITE
                && (url.path() == "/entry"
                    || (hop > 0 && url.path().starts_with("/frontend/evo/"))),
            "unreviewed demo redirect"
        );
        if url.path().starts_with("/frontend/evo/") {
            // The official redirect carries browser-side state in the fragment.
            // It is not part of the HTTP request target or the Referer.
            url.set_fragment(None);
        }
        let response = ctx
            .http(
                "demo_entry",
                "GET",
                url.as_str(),
                BTreeMap::from([
                    ("referer".into(), launch.clone()),
                    (
                        "accept".into(),
                        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".into(),
                    ),
                    ("sec-fetch-mode".into(), "navigate".into()),
                    ("sec-fetch-dest".into(), "iframe".into()),
                    ("user-agent".into(), CLIENT_UA.into()),
                ]),
                String::new(),
                false,
            )
            .await?;
        if response.status == 200 {
            reached = url.path().starts_with("/frontend/evo/");
            break;
        }
        ensure!(
            matches!(response.status, 301 | 302 | 303 | 307 | 308),
            "unconfirmed demo entry"
        );
        url = url.join(
            response
                .headers
                .get("location")
                .context("missing native redirect")?,
        )?;
    }
    ensure!(reached, "official frontend not reached");
    let headers = BTreeMap::from([
        ("referer".into(), url.to_string()),
        ("accept".into(), "*/*".into()),
        ("accept-language".into(), "es".into()),
        ("user-agent".into(), CLIENT_UA.into()),
    ]);
    let setup = ctx
        .http(
            "setup",
            "GET",
            &format!(
                "{FANSITE}/setup?device=desktop&wrapped=false&locale=en-GB&client_version={VERSION}"
            ),
            headers.clone(),
            String::new(),
            false,
        )
        .await?;
    ensure!(setup.status == 200, "native setup unavailable");
    let config = ctx
        .http(
            "configuration",
            "GET",
            &format!("{FANSITE}/config?table_id={table}&client_version={VERSION}"),
            headers,
            String::new(),
            false,
        )
        .await?;
    ensure!(config.status == 200, "native configuration unavailable");
    let cfg: Value = serde_json::from_str(&config.body)?;
    let w = &cfg["wrappedLaunchParams"];
    let id = text(w, "gameId")?;
    ensure!(
        cfg["table_id"] == table.as_str() && cfg["base_game_id"] == id,
        "native game binding changed"
    );
    let base = text(w, "baseUrl")?.trim_end_matches('/');
    let target = Url::parse(base)?;
    ensure!(
        target.scheme() == "https"
            && target.port().is_none()
            && target.username().is_empty()
            && target.password().is_none()
            && target
                .host_str()
                .is_some_and(|h| h.ends_with(".redtiger.cash"))
            && target.query().is_none()
            && target.fragment().is_none(),
        "invalid settings origin"
    );
    let platform = format!("{base}/platform/game");
    let mut params = json!({"token":null,"sessionId":text(w,"token")?,"gameId":id,"playMode":"demo","listenToFrontend":true,
      "custom":{"siteId":w["casino"],"context":{"ne_evo_token":text(w,"ne_evo_token")?}},
      "userData":{"userId":"0","fingerprint":Uuid::new_v4().simple().to_string(),"lang":w["lang"],"channel":w["channel"]}});
    let native_headers = BTreeMap::from([
        ("origin".into(), target.origin().ascii_serialization()),
        ("user-agent".into(), CLIENT_UA.into()),
    ]);
    let settings = ctx
        .json(
            "settings",
            &format!("{platform}/settings"),
            &params,
            native_headers.clone(),
            false,
        )
        .await?;
    let user = &settings["result"]["user"];
    ensure!(
        settings["success"] == true && !user["userId"].is_null(),
        "demo settings not confirmed"
    );
    let token = text(user, "token")?;

    let stake = user["stakes"]["types"]
        .as_array()
        .context("native stake menu absent")?
        .iter()
        .map(money)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|v| *v > Decimal::ZERO)
        .min()
        .context("no positive stake offered")?;

    let choice = reviewed_choice(game, mode);
    let native_mode: u64 = if choice.is_some() {
        101
    } else {
        mode.key.parse()?
    };
    validate_math_mode(&settings["result"]["game"], native_mode, &mode.kind)?;

    params["token"] = json!(token);
    params["userData"]["userId"] = user["userId"].clone();
    ctx.save_session(&params)?;
    let mut entry = params.clone();
    entry["stake"] = serde_json::from_str(&stake.to_string())?;
    entry["bonusId"] = Value::Null;
    entry["gameMode"] = json!(native_mode);
    let spin = ctx
        .json(
            "spin",
            &format!("{platform}/spin"),
            &entry,
            native_headers.clone(),
            true,
        )
        .await?;
    ensure!(spin["success"] == true, "native spin rejected");
    let result = &spin["result"];
    same_user(result, &settings["result"])?;
    let round = result["transactions"]["roundId"].clone();
    ensure!(
        round.is_string() || round.is_number(),
        "native round id missing"
    );
    let mut current = result.clone();
    if let Some(choice) = choice {
        ensure!(
            current["game"]["hasState"] == true
                && available_choices(&current["game"])?.is_some_and(|options| options
                    .iter()
                    .filter(|option| **option == choice)
                    .count()
                    == 1),
            "requested native choice not offered; retain round"
        );
        params["roundId"] = round.clone();
        params["sessionId"] = json!(text(&current["user"], "sessionId")?);
        let mut command = params.clone();
        command["choice"] = json!(choice);
        if choice == "WildMultiplier" {
            command["gameMode"] = json!(native_mode);
        }
        let response = ctx
            .json(
                "choice",
                &format!("{platform}/choice"),
                &command,
                native_headers.clone(),
                true,
            )
            .await?;
        let next = &response["result"];
        same_user(next, &current)?;
        let selected = next["game"]["choices"]["selected"].as_str();
        ensure!(
            response["success"] == true
                && next["transactions"]["roundId"] == round
                && (selected == Some(choice)
                    || (choice == "Random"
                        && matches!(selected, Some("WildMultiplier" | "WaysMultiplier")))),
            "native choice was not confirmed on the same round"
        );
        current = next.clone();
        params["gameMode"] = json!(native_mode);
    }
    match current["game"]["hasState"].as_bool() {
        Some(false) => {}
        Some(true) => {
            if available_choices(&current["game"])?.is_some() {
                return Err(unknown_event("Red Tiger unrecognized native choice"));
            }
            params["roundId"] = round.clone();
            if let Some(session) = current["user"]["sessionId"]
                .as_str()
                .filter(|s| !s.is_empty())
            {
                params["sessionId"] = json!(session);
            }
            let finish = ctx
                .json(
                    "finish",
                    &format!("{platform}/finish"),
                    &params,
                    native_headers,
                    true,
                )
                .await?;
            finish_ack(&finish, &round, &current)?;
        }
        None => return Err(unknown_event("Red Tiger native closure unavailable")),
    }
    Ok(Completion {
        native_round_id: Some(
            round
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| round.to_string()),
        ),
        details: json!({"game":id,"choice":choice,"native_choice":current["game"]["choices"]["selected"]}),
    })
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn math_mode_binding_needs_only_native_id_and_type() {
        let menu = json!({"mathModes":[{"id":101,"type":"featureBuy"},{"id":2,"type":"chanceIncrease"},{"id":3,"type":"stateSwitch"}]});
        validate_math_mode(&menu, 101, &ModeKind::Purchase).unwrap();
        validate_math_mode(&menu, 2, &ModeKind::AnteBet).unwrap();
        validate_math_mode(&menu, 3, &ModeKind::Variant).unwrap();
        validate_math_mode(&json!({}), 0, &ModeKind::Normal).unwrap();
        assert!(validate_math_mode(&menu, 101, &ModeKind::AnteBet).is_err());
        assert!(validate_math_mode(&menu, 999, &ModeKind::Purchase).is_err());
    }
    #[test]
    fn native_finish_without_balances_or_awards() {
        let previous = json!({"user":{"userId":7,"sessionId":"s"}});
        let finish = json!({"success":true,"result":{"transactions":{"roundId":"r"},"user":{"userId":7,"sessionId":"s"}}});
        assert!(finish_ack(&finish, &json!("r"), &previous).unwrap());
    }
    #[test]
    fn unknown_choice_shape_is_typed() {
        assert!(
            available_choices(&json!({"choices":"new"}))
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
