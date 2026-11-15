//! Native capture protocol derived from standalone info/hacksaw/codigo and the evidence reference adapter.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
fn id(v: &Value) -> Result<String> {
    match v {
        Value::String(s) if !s.is_empty() => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        _ => bail!("missing native identifier"),
    }
}
fn offer(auth: &Value, key: &str) -> Result<()> {
    let offers = auth["bonusGames"]
        .as_array()
        .context("missing native offers")?;
    let mut matching = offers
        .iter()
        .filter(|v| v["bonusGameId"].as_str() == Some(key));
    matching
        .next()
        .context("inventory mode not offered in fresh session")?;
    ensure!(matching.next().is_none(), "duplicate native offer");
    Ok(())
}

async fn call(ctx: &RunContext, endpoint: &str, body: Value, financial: bool) -> Result<Value> {
    let response = ctx
        .json(
            endpoint,
            &format!("https://rgs-demo.hacksawgaming.com/api/play/{endpoint}"),
            &body,
            BTreeMap::new(),
            financial,
        )
        .await?;
    ensure!(
        response["statusCode"].as_i64() == Some(0),
        "Hacksaw provider error: {}",
        response["statusCode"]
    );
    Ok(response)
}
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    ensure!(
        !game.id.is_empty() && game.id.bytes().all(|b| b.is_ascii_digit()),
        "invalid game id"
    );
    let gid: u64 = game.id.parse()?;
    let variant = mode.key == "mode-extreme" && game.id == "1426";
    match mode.kind {
        ModeKind::Normal => ensure!(mode.key == "normal", "unsupported normal selector"),
        ModeKind::Purchase | ModeKind::Booster if !variant => {}
        ModeKind::Variant if variant => {}
        ModeKind::Purchase if variant => {}
        _ => bail!("unsupported Hacksaw mode"),
    }
    let version_response = ctx
        .get(
            "version",
            &format!(
                "https://static-live.hacksawgaming.com/{}/version.json",
                game.id
            ),
        )
        .await?;
    ensure!(
        (200..300).contains(&version_response.status),
        "version HTTP error"
    );
    let version_json: Value = serde_json::from_str(&version_response.body)?;
    let version = version_json["version"]
        .as_str()
        .context("missing version")?;
    let parts: Vec<_> = version.split('.').collect();
    ensure!(
        (2..=4).contains(&parts.len())
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
        "invalid published version"
    );
    let settings = call(
        ctx,
        "partnerSettings",
        json!({"partner":"demo","currency":"EUR","gameId":gid,"usePartnerCurrency":true}),
        false,
    )
    .await?;
    let auth = call(ctx, "authenticate", json!({"seq":1,"partner":"demo","gameId":gid,"gameVersion":version,"currency":"EUR","languageCode":"en-us","mode":2,"branding":"default","channel":1,"userAgent":"Mozilla/5.0","token":"demo","currencyMultiplier":1}), false).await?;
    ensure!(
        auth["name"] == "Demo"
            && (auth["roundStatus"].is_null()
                || auth["roundStatus"] == false
                || auth["roundStatus"] == ""
                || auth["roundStatus"] == 0)
            && (auth["roundId"].is_null() || auth["roundId"] == "0" || auth["roundId"] == 0),
        "not a fresh public demo"
    );
    ensure!(
        id(&auth["gameId"])? == game.id,
        "authenticated game id mismatch"
    );
    let session = auth["sessionUuid"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("missing session")?;

    let bets = auth["betLevels"]
        .as_array()
        .context("missing published bets")?;
    let bet = bets
        .iter()
        .map(amount)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .min()
        .context("empty published bets")?;
    ensure!(
        bet > Decimal::ZERO && bet.fract() == Decimal::ZERO,
        "invalid published bet"
    );
    let bonus = if mode.kind != ModeKind::Normal && !variant {
        Some(mode.key.as_str())
    } else {
        None
    };
    if let Some(key) = bonus {
        offer(&auth, key)?;
    }

    let mut wager = json!({"betAmount":bet.to_string()});
    if let Some(key) = bonus {
        wager["buyBonus"] = json!(key);
    }
    if game.id == "1426" {
        let volatility = if variant {
            "extreme"
        } else if let Some(key) = bonus {
            key.rsplit('_').next().unwrap_or("")
        } else {
            "low"
        };
        ensure!(
            volatility == "low" || volatility == "extreme",
            "unpublished volatility selector"
        );
        wager["customData"] = json!({"volatility":volatility});
    }
    ctx.save_session(&json!({"provider":"hacksaw","game":game.id,"version":version,"sessionUuid":session,"currency":"EUR"}))?;
    let mut seq = 2u64;
    let mut response = call(ctx, "bet", json!({"seq":seq,"sessionUuid":session,"bets":[wager],"offerId":null,"promotionId":null,"autoplay":false}), true).await?;
    let round_id = id(&response["round"]["roundId"])?;
    let mut responses = vec![response.clone()];
    let mut closed = false;
    for _ in 0..ctx.max_steps() {
        let round = &response["round"];
        ensure!(id(&round["roundId"])? == round_id, "round identity changed");
        if native_closed(round)? {
            closed = true;
            break;
        }
        let action = next_action(round)?;
        seq += 1;
        response = call(ctx, "bet", json!({"seq":seq,"sessionUuid":session,"roundId":round["roundId"],"continueInstructions":{"action":action}}), true).await?;
        responses.push(response.clone());
    }
    // Inspect the response to the final permitted send before declaring exhaustion.
    if !closed {
        closed = native_closed(&response["round"])?;
    }
    ensure!(closed, "Hacksaw continuation limit reached");
    Ok(Completion {
        native_round_id: Some(round_id),
        details: json!({"version":version,"settings":settings,"authentication":auth,"bet":bet.to_string(),"responses":responses}),
    })
}

fn native_closed(round: &Value) -> Result<bool> {
    if round["status"] != "completed" {
        return Ok(false);
    }
    if !round["possibleActions"].is_null()
        && !round["possibleActions"]
            .as_array()
            .is_some_and(|v| v.is_empty())
    {
        return Err(unknown_event(
            "Hacksaw completed round advertises pending actions",
        ));
    }
    Ok(true)
}
fn next_action(round: &Value) -> Result<&str> {
    // The public client explicitly acknowledges this presentation state. Other native controls
    // require their own proven request binding; an arbitrary server string is never replayed.
    if round["status"] == "wfwpc" {
        let actions = &round["possibleActions"];
        if actions.is_null()
            || actions.as_array().is_some_and(|a| {
                a.is_empty() || (a.len() == 1 && a[0] == "win_presentation_complete")
            })
        {
            return Ok("win_presentation_complete");
        }
    }
    if let Some(actions) = round["possibleActions"].as_array()
        && actions.len() == 1
        && actions[0] == "win_presentation_complete"
    {
        return Ok("win_presentation_complete");
    }
    Err(unknown_event(format!(
        "Hacksaw unsupported next state: {}",
        round
    )))
}
pub fn supports(game: &Game, mode: &Mode) -> bool {
    !game.id.is_empty()
        && game.id.bytes().all(|b| b.is_ascii_digit())
        && match mode.kind {
            ModeKind::Normal => mode.key == "normal",
            ModeKind::Booster | ModeKind::Purchase => !mode.key.is_empty(),
            ModeKind::Variant => game.id == "1426" && mode.key == "mode-extreme",
            _ => false,
        }
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn closes_without_wallet_or_award_fields() {
        let round = json!({"roundId":"r","status":"completed","possibleActions":[]});
        assert!(native_closed(&round).unwrap());
    }
    #[test]
    fn only_native_acknowledgement_is_replayed() {
        assert_eq!(
            next_action(&json!({"status":"wfwpc"})).unwrap(),
            "win_presentation_complete"
        );
        for round in [
            json!({"status":"new_state","possibleActions":["invented"]}),
            json!({"status":"completed","possibleActions":["new"]}),
        ] {
            let err = if round["status"] == "completed" {
                native_closed(&round).unwrap_err()
            } else {
                next_action(&round).unwrap_err()
            };
            assert!(err.downcast_ref::<crate::model::UnknownEvent>().is_some());
        }
    }
}
