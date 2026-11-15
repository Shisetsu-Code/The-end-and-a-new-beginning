//! Native capture protocol derived from standalone info/peterandsons/codigo and the evidence reference adapter.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, exact_div100, exact_mul, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn anonymous_key_below(upper: u64, mut entropy: impl FnMut() -> u64) -> Result<String> {
    ensure!(upper > 0, "invalid anonymous launch key range");
    // Reject the short residue prefix so every value below upper is equally likely.
    let threshold = upper.wrapping_neg() % upper;
    loop {
        let candidate = entropy();
        if candidate >= threshold {
            return Ok((candidate % upper).to_string());
        }
    }
}

fn anonymous_key() -> Result<String> {
    anonymous_key_below(crate::model::now_ms(), || {
        let bytes = uuid::Uuid::new_v4().into_bytes();
        // These eight UUIDv4 bytes contain only random bits, excluding version/variant.
        u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[14], bytes[15],
        ])
    })
}

fn money(v: &Value) -> Result<Decimal> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => bail!("missing or invalid native amount"),
    };
    let n = decimal(&s)?;
    ensure!(n >= Decimal::ZERO, "negative native amount");
    Ok(n)
}
fn required<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v.get(k)
        .and_then(Value::as_str)
        .filter(|x| !x.is_empty())
        .with_context(|| format!("missing {k}"))
}
fn native_price(info: &Value, action: &str, kind: ModeKind) -> Result<Decimal> {
    ensure!(
        action
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
            && !action.is_empty(),
        "unsafe native action"
    );
    let compatible = match kind {
        ModeKind::Normal => action == "main",
        ModeKind::Purchase => action == "buy" || action.starts_with("buy_"),
        ModeKind::AnteBet | ModeKind::Special => action == "bet" || action.starts_with("bet_"),
        _ => false,
    };
    ensure!(compatible, "inventory mode does not match native action");
    if action == "buy" {
        ensure!(
            money(
                info.pointer("/config/buy_configuration/BUY")
                    .context("missing buy configuration")?
            )? > Decimal::ZERO,
            "buy configuration not offered"
        );
    }
    if action == "bet" {
        ensure!(
            money(
                info.pointer("/config/special_bets_configuration/BET")
                    .context("missing bet configuration")?
            )? > Decimal::ZERO,
            "bet configuration not offered"
        );
    }
    let values = info
        .pointer(&format!("/bets/{action}/available"))
        .and_then(Value::as_array)
        .context("native action not offered")?;
    let parsed = values.iter().map(money).collect::<Result<Vec<_>>>()?;
    let price = *parsed.iter().min().context("empty native offer")?;
    ensure!(price > Decimal::ZERO, "invalid price");
    Ok(price)
}

fn conditional_price(info: &Value, packet: &Value, game: &str, action: &str) -> Result<Decimal> {
    ensure!(
        game == "bloody-mary" && matches!(action, "extra_spin_fs" | "extra_spin_sfs"),
        "conditional action outside reviewed contract"
    );
    let state = if action == "extra_spin_fs" {
        "SESSION_FS"
    } else {
        "SESSION_SFS"
    };
    let step = packet
        .pointer("/wager/data")
        .and_then(Value::as_array)
        .and_then(|v| v.last())
        .context("conditional step missing")?;
    ensure!(step["state"] == state, "conditional state mismatch");
    let offered = step["pending_actions"]
        .as_array()
        .context("conditional offer missing")?
        .iter()
        .filter(|v| v["name"] == action && v["kind"] == "Buy")
        .collect::<Vec<_>>();
    let configured = info
        .pointer(&format!("/config/per_state/{state}/continuation_offers"))
        .and_then(Value::as_array)
        .context("conditional configuration missing")?
        .iter()
        .flat_map(|v| v["actions"].as_array().into_iter().flatten())
        .filter(|v| v["name"] == action && v["kind"] == "Buy")
        .collect::<Vec<_>>();
    ensure!(
        offered.len() == 1
            && configured.len() == 1
            && offered[0]["display"] == configured[0]["display"]
            && offered[0]["payload"] == configured[0]["payload"],
        "conditional offer configuration mismatch"
    );
    let display = &offered[0]["display"];
    let counter_name = required(display, "cost_counter")?;
    let counter = step["counters"]
        .get(counter_name)
        .context("conditional cost counter missing")?;
    let base = money(&step["bet_amount"])?;
    let base_key = match &step["bet_amount"] {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    let index = counter["value_per_bet"]
        .get(&base_key)
        .unwrap_or(&counter["default_value"])
        .as_u64()
        .context("conditional cost counter invalid")? as usize;
    let factor = display["cost_table"]
        .as_array()
        .and_then(|x| x.get(index))
        .context("conditional cost table missing")?;
    let cost = money(&offered[0]["cost"])?;
    ensure!(
        cost > Decimal::ZERO && cost == exact_mul(base, money(factor)?)? && cost.fract().is_zero(),
        "conditional native price mismatch"
    );
    exact_div100(cost)
}

/// `wallet` is the nonsecret wallet identifier from the official public catalogue;
/// Wallet values are public launch identifiers, unrelated to monetary validation.
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    let (parent, conditional) = match mode.key.split_once("::offer::") {
        Some((p, a)) => {
            ensure!(
                mode.kind == ModeKind::Purchase,
                "conditional classification mismatch"
            );
            (p, Some(a))
        }
        None => (mode.key.as_str(), None),
    };
    let wallet = required(settings, "wallet")
        .context("settings.wallet required from official public catalogue")?;
    let key = anonymous_key()?;
    let api = "https://demo.spinart.cloud";
    let mut headers = BTreeMap::new();
    headers.insert("Origin".into(), "https://cdn-demo.spinart.cloud".into());
    headers.insert("Referer".into(), "https://cdn-demo.spinart.cloud/".into());
    let cfg=ctx.json("configuration",&format!("{api}/cfg"),&json!({"server":api,"wallet":wallet,"operator":"demo","gameCode":game.id,"key":key,"language":"en"}),headers.clone(),false).await?;
    ensure!(
        cfg["adapter"] == "standard" && cfg["loadGame"] == true,
        "unsupported demo configuration"
    );
    let auth=ctx.json("authentication",&format!("{api}/authenticate"),&json!({"wallet":wallet,"operator":"demo","key":key,"provider":"peterandsons","game":game.id}),headers.clone(),false).await?;

    let token = required(&auth, "token")?;
    headers.insert("Authorization".into(), format!("Bearer {token}"));
    let common = json!({"game":game.id,"provider":"peterandsons"});
    let recover = ctx
        .json(
            "recovery",
            &format!("{api}/game/recover"),
            &json!({"game":game.id,"provider":"peterandsons","complete":false}),
            headers.clone(),
            false,
        )
        .await?;
    ensure!(
        recover["rounds"].as_array().is_some_and(|v| v.is_empty()),
        "fresh demo has pending rounds"
    );
    let info = ctx
        .json(
            "information",
            &format!("{api}/game/info"),
            &common,
            headers.clone(),
            false,
        )
        .await?;
    let parent_kind = if parent == "main" {
        ModeKind::Normal
    } else if parent.starts_with("buy_") || parent == "buy" {
        ModeKind::Purchase
    } else {
        ModeKind::AnteBet
    };
    let price = native_price(
        &info,
        parent,
        if conditional.is_some() {
            parent_kind
        } else {
            mode.kind
        },
    )?;

    ctx.save_session(
        &json!({"provider":"peterandsons","game":game.id,"wallet":wallet,"key":key,"token":token}),
    )?;
    let bet: Value = serde_json::from_str(&price.to_string())?;
    let request = json!({"game":game.id,"provider":"peterandsons","action":parent,"complete":false,"asyncWin":false,"bet":bet});
    let first = ctx
        .json(
            "play",
            &format!("{api}/game/play"),
            &request,
            headers.clone(),
            true,
        )
        .await?;
    let id = required(&first, "roundId")?.to_owned();

    let mut packets = vec![first];

    let mut selected = false;
    for _ in 0..ctx.max_steps() {
        let current = packets.last().unwrap();
        let next = current
            .pointer("/wager/next")
            .and_then(Value::as_array)
            .ok_or_else(|| unknown_event("missing next actions"))?;
        if next.is_empty() {
            break;
        }
        if next.iter().any(|v| {
            !matches!(v.as_str(), Some("take_round"))
                && !(game.id == "bloody-mary"
                    && matches!(v.as_str(), Some("extra_spin_fs" | "extra_spin_sfs")))
        }) {
            return Err(unknown_event(format!(
                "Peter & Sons unknown next action: {next:?}"
            )));
        }
        if let Some(action) = conditional
            && !selected
            && next.iter().any(|x| x == action)
        {
            let extra = conditional_price(&info, current, &game.id, action)?;

            let amount: Value = serde_json::from_str(&extra.to_string())?;
            let request = json!({"game":game.id,"provider":"peterandsons","action":action,"complete":false,"asyncWin":false,"bet":amount,"roundId":id});
            let reply = ctx
                .json(
                    "conditional_purchase",
                    &format!("{api}/game/play"),
                    &request,
                    headers.clone(),
                    true,
                )
                .await?;
            ensure!(
                required(&reply, "roundId")? == id,
                "conditional owner changed"
            );

            packets.push(reply);
            selected = true;
            continue;
        }
        if !(next.iter().any(|v| v == "take_round")) {
            return Err(unknown_event("unreviewed pending round or choice"));
        }
        let continuation = json!({"game":game.id,"provider":"peterandsons","action":"take_round","complete":false,"asyncWin":false,"roundId":id});
        let reply = ctx
            .json(
                "take_round",
                &format!("{api}/game/play"),
                &continuation,
                headers.clone(),
                true,
            )
            .await?;
        ensure!(
            required(&reply, "roundId")? == id,
            "continuation owner changed"
        );
        packets.push(reply);
    }
    let requested_offer_seen = conditional.is_none() || selected;
    validate_closed(packets.last().context("no Peter & Sons packets")?)?;
    let completion = ctx
        .json(
            "complete",
            &format!("{api}/game/complete"),
            &json!({"game":game.id,"provider":"peterandsons","roundId":id,"asyncWin":false}),
            headers,
            true,
        )
        .await?;
    Ok(Completion {
        native_round_id: Some(id),
        details: json!({"information":info,"play_request":request,"packets":packets,"completion":completion,"requested_offer_seen":requested_offer_seen}),
    })
}

fn validate_closed(packet: &Value) -> Result<()> {
    let wager = &packet["wager"];
    if !wager["next"].as_array().is_some_and(|a| a.is_empty())
        || wager["data"]
            .as_array()
            .and_then(|a| a.last())
            .and_then(|s| s["closes_game_round"].as_bool())
            != Some(true)
    {
        return Err(unknown_event(
            "Peter & Sons native round has no recognized closure",
        ));
    }
    Ok(())
}
pub fn supports(game: &Game, mode: &Mode) -> bool {
    if let Some((_, offer)) = mode.key.split_once("::offer::") {
        return game.id == "bloody-mary"
            && mode.kind == ModeKind::Purchase
            && matches!(offer, "extra_spin_fs" | "extra_spin_sfs");
    }
    match mode.kind {
        ModeKind::Normal => mode.key == "main",
        ModeKind::Purchase => mode.key == "buy" || mode.key.starts_with("buy_"),
        ModeKind::AnteBet | ModeKind::Special => mode.key == "bet" || mode.key.starts_with("bet_"),
        _ => false,
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn native_close_does_not_need_money_fields() {
        validate_closed(
            &json!({"roundId":"r","wager":{"next":[],"data":[{"closes_game_round":true}]}}),
        )
        .unwrap();
    }
    #[test]
    fn open_or_unknown_next_is_not_completion() {
        for packet in [
            json!({"wager":{"next":["new_control"],"data":[{"closes_game_round":true}]}}),
            json!({"wager":{"next":[],"data":[{"closes_game_round":false}]}}),
        ] {
            assert!(
                validate_closed(&packet)
                    .unwrap_err()
                    .downcast_ref::<crate::model::UnknownEvent>()
                    .is_some()
            );
        }
    }
}
