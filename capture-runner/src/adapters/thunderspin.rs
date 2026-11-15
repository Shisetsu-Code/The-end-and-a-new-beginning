//! Native capture protocol derived from standalone info/thunderspin/codigo and the evidence reference adapter.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, exact_mul, now_ms, sha256, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock};
use url::Url;

fn binding(game: &Game, mode: &Mode) -> Option<&'static Value> {
    static MAP: OnceLock<Value> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        serde_json::from_str(include_str!("../../bindings/thunderspin.json"))
            .expect("embedded ThunderSpin catalogue")
    });
    let row = map.get(&game.id)?;
    (matches!(
        (mode.key.as_str(), mode.kind),
        ("NORMAL", ModeKind::Normal) | ("BUY_BONUS", ModeKind::Purchase)
    ) && row["modes"].as_array()?.contains(&json!(mode.key)))
    .then_some(row)
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    binding(game, mode).is_some()
}

fn amount(value: &Value) -> Result<Decimal> {
    let value = match value {
        Value::String(s) => decimal(s)?,
        Value::Number(n) => decimal(&n.to_string())?,
        _ => bail!("ThunderSpin native amount missing"),
    };
    ensure!(value >= Decimal::ZERO, "negative ThunderSpin native amount");
    Ok(value)
}

fn minimum(values: &Value) -> Result<&Value> {
    let mut offers = Vec::new();
    for value in values.as_array().context("ThunderSpin offer missing")? {
        let price = amount(value)?;
        if price > Decimal::ZERO {
            offers.push((price, value));
        }
    }
    offers
        .into_iter()
        .min_by_key(|offer| offer.0)
        .map(|offer| offer.1)
        .context("ThunderSpin positive offer missing")
}

async fn post(
    ctx: &RunContext,
    service: &str,
    action: &str,
    fields: &Value,
    financial: bool,
) -> Result<Value> {
    let body = {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in fields
            .as_object()
            .context("ThunderSpin form is not an object")?
        {
            let repr = value.to_string();
            form.append_pair(key, value.as_str().unwrap_or(&repr));
        }
        form.finish()
    };
    let response = ctx
        .http(
            action,
            "POST",
            &format!("{service}/{action}"),
            BTreeMap::from([(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            )]),
            body,
            financial,
        )
        .await?;
    ensure!(
        response.status == 200,
        "ThunderSpin HTTP {} at {action}",
        response.status
    );
    let value: Value = serde_json::from_str(&response.body)?;
    let empty = |v: &Value| v.is_null() || v == false || v == 0 || v == "";
    ensure!(
        empty(&value["Error"])
            && empty(&value["ErrorCode"])
            && value["status"]["code"]
                .as_u64()
                .is_none_or(|code| code < 400),
        "ThunderSpin native command rejected"
    );
    Ok(value)
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &serde_json::Value,
) -> Result<Completion> {
    let row = binding(game, mode).context("ThunderSpin mode missing from embedded catalogue")?;
    let url = Url::parse(row["url"].as_str().context("ThunderSpin launch missing")?)?;
    let html = ctx.get("demo_page", url.as_str()).await?;
    ensure!(html.status == 200, "ThunderSpin demo page unavailable");
    let scripts = regex::Regex::new(r#"(?i)<script[^>]+src=["']([^"']+)"#)?;
    let bundle = regex::Regex::new(r"^index(?:\.[a-f0-9]+)?(?:\.chunk)?\.js$")?;
    let mut candidates = Vec::new();
    for found in scripts.captures_iter(&html.body) {
        let target = url.join(&found[1])?;
        if target.origin() == url.origin()
            && target
                .path_segments()
                .and_then(|mut s| s.next_back())
                .is_some_and(|file| bundle.is_match(file))
        {
            candidates.push(target);
        }
    }
    ensure!(
        candidates.len() == 1,
        "ThunderSpin public client bundle missing or ambiguous"
    );
    let client = ctx.get("native_client", candidates[0].as_str()).await?;
    ensure!(client.status == 200, "public client source HTTP error");
    if !(row["hash"] == sha256(client.body.as_bytes())) {
        return Err(unknown_event("ThunderSpin client revision changed"));
    }
    let game_code = row["slotcode"]
        .as_str()
        .context("ThunderSpin native game code missing")?;
    let service = row["service"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}/service", url.origin().ascii_serialization()));
    let sid = format!("dev_{}", now_ms());
    let init = post(
        ctx,
        &service,
        "_Initialise",
        &json!({"sid":sid,"slotcode":game_code}),
        false,
    )
    .await?;
    let balance = &init["Balance"];
    ensure!(
        balance["Demo"] == true
            && init["LastResult"].is_null()
            && init["GameCode"].as_str().is_none_or(|id| id == game_code),
        "ThunderSpin initial demo state not idle"
    );
    let denom = minimum(&balance["CoinValue"])?;
    let level = minimum(&balance["BetLevel"])?;
    let base = exact_mul(
        exact_mul(
            amount(init.get("DefaultBet").unwrap_or(&balance["DefaultBet"]))?,
            amount(denom)?,
        )?,
        amount(level)?,
    )?;
    let buy = mode.kind == ModeKind::Purchase;
    if buy {
        let offer = &init["Features"]["BuyBonus"]["Costs"]["costs"][base.normalize().to_string()];
        ensure!(
            offer["allowed"] == true
                && (row["buy_fixed_level_one"] != true || amount(level)? == Decimal::ONE),
            "ThunderSpin native purchase unavailable"
        );
    }

    ctx.save_session(
        &json!({"provider":"thunderspin","slotcode":game_code,"sid":sid,"service":service}),
    )?;
    let mut action = if buy { "_BuyBonus" } else { "_Roll" };
    let mut expected_mode = "root".to_owned();
    let mut lines = balance
        .get("CurrentLinesCount")
        .cloned()
        .unwrap_or_else(|| json!(0));

    for step in 0..ctx.max_steps().min(500) {
        let mut fields = json!({"sid":sid});
        if action != "_AcceptWin" {
            fields = json!({"sid":sid,"denomination":denom,"betlevel":level,"data":0});
            if action == "_BuyBonus" && row["buy_lines"] == true {
                fields["lines"] = json!(init["Lines"].as_array().map_or(0, Vec::len));
            } else if action == "_Roll" && row["roll_lines"] == true {
                fields["lines"] = lines.clone();
            }
        }
        let response = post(ctx, &service, action, &fields, true).await?;
        let closed = if action == "_AcceptWin" {
            true
        } else {
            if response["Mode"] != expected_mode {
                return Err(unknown_event("ThunderSpin unexpected native mode"));
            }
            let (next, closed) = native_state(&response, &init, row)?;
            expected_mode = next.to_owned();
            let root =
                expected_mode == "root" && response["FreeSpinsLeft"].as_u64().unwrap_or(0) == 0;
            let accept = &response["waitForAcceptWin"];
            if !closed {
                if root
                    && accept == true
                    && client
                        .body
                        .contains("name:\"AcceptWin\",body:{sid:app.request.sid}")
                {
                    action = "_AcceptWin";
                } else if accept != true
                    && row["auto_roll_modes"]
                        .as_array()
                        .is_some_and(|modes| modes.contains(&json!(expected_mode)))
                {
                    action = "_Roll";
                    lines = response["LinesCount"].clone();
                } else {
                    return Err(unknown_event(
                        "ThunderSpin requires an unimplemented native continuation",
                    ));
                }
            }
            closed
        };
        if closed {
            return Ok(Completion {
                native_round_id: None,
                details: json!({"game":game_code,"steps":step+1}),
            });
        }
    }
    bail!("ThunderSpin continuation limit reached")
}

fn native_state<'a>(response: &'a Value, init: &Value, row: &Value) -> Result<(&'a str, bool)> {
    let mode = response["NextMode"]
        .as_str()
        .ok_or_else(|| unknown_event("ThunderSpin next mode missing"))?;
    if mode != "root"
        && !row["auto_roll_modes"]
            .as_array()
            .is_some_and(|v| v.contains(&json!(mode)))
    {
        return Err(unknown_event(format!(
            "ThunderSpin unsupported next mode: {mode}"
        )));
    }
    let root = mode == "root" && response["FreeSpinsLeft"].as_u64().unwrap_or(0) == 0;
    let accept = &response["waitForAcceptWin"];
    let closed = root
        && (accept == false
            || (accept.is_null() && init["GameSettings"]["IsGambleAllowed"] == false));
    Ok((mode, closed))
}
#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn native_closure_needs_no_balance() {
        assert_eq!(
            native_state(
                &json!({"NextMode":"root","FreeSpinsLeft":0,"waitForAcceptWin":false}),
                &json!({}),
                &json!({})
            )
            .unwrap(),
            ("root", true)
        );
    }
    #[test]
    fn native_continuation_is_bound_to_published_modes() {
        assert_eq!(
            native_state(
                &json!({"NextMode":"fs","FreeSpinsLeft":2}),
                &json!({}),
                &json!({"auto_roll_modes":["fs"]})
            )
            .unwrap(),
            ("fs", false)
        );
        assert!(
            native_state(&json!({"NextMode":"new"}), &json!({}), &json!({}))
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
