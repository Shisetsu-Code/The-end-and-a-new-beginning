//! Native protocol reconstructed from standalone info/microgaming/codigo; no old runtime dependency.
//! A single fresh 3rd Base practice round, using the published HTTP contract.
//! Source: bet-collector/collection_claude_extra.py and microgaming-bindings.json.
use crate::{context::RunContext, model::*};
use anyhow::{Context, Result, ensure};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

const GAME: &str = "3rd-base";
const MODULE: u64 = 11194;
const CLIENT: u64 = 50300;
const PRODUCT: u64 = 37241;
const API: &str = "https://api8888.maf-114.xyz/base";
const ORIGIN: &str = "https://luckyshow.contdelivery.com";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:159.0) Gecko/20100101 Firefox/159.0";

struct Profile {
    token: String,
    raw_stake: Value,
}

pub fn validate(game: &Game, mode: &Mode, settings: &Value) -> Result<()> {
    ensure!(
        game.id == GAME && mode.kind == ModeKind::Normal && mode.key == "normal",
        "Microgaming supports only the reviewed 3rd Base normal entry"
    );
    ensure!(
        settings.is_null() || settings.as_object().is_some_and(|o| o.is_empty()),
        "Microgaming public binding has no overrides"
    );
    Ok(())
}

fn amount(value: &Value) -> Result<Decimal> {
    ensure!(
        value.is_number() || value.is_string(),
        "Microgaming native amount missing"
    );
    decimal(
        &value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string()),
    )
}

fn token(practice: &Value) -> Result<&str> {
    practice["tokens"]["userToken"]
        .as_str()
        .filter(|s| !s.is_empty() && *s != "[REDACTED]" && s.bytes().all(|b| b.is_ascii_graphic()))
        .context("Microgaming fresh practice token missing")
}

fn profile(practice: &Value, initial: &Value) -> Result<Profile> {
    let token = token(practice)?.to_owned();

    let context = &initial["context"];
    let game = &initial["gameResponse"];
    let (raw_stake, stake) = selected_bet(&game["financials"]["availableBetLimits"])?;
    ensure!(
        game["summary"]["isComplete"] == true
            && context["isReadOnly"] == true
            && context["game"]["transactionNumber"] == -1
            && stake > Decimal::ZERO,
        "Microgaming practice session is not fresh and idle"
    );
    Ok(Profile { token, raw_stake })
}

fn selected_bet(menu: &Value) -> Result<(Value, Decimal)> {
    let default = &menu["defaultBet"];
    if !default.is_object() {
        return Ok((default.clone(), amount(default)?));
    }
    let minimum = amount(&menu["minBet"])?;
    let available = menu["availableBets"]
        .as_array()
        .context("Microgaming total bet menu missing")?;
    ensure!(
        minimum > Decimal::ZERO
            && available
                .iter()
                .any(|v| amount(v).is_ok_and(|v| v == minimum)),
        "Microgaming minimum total bet not offered"
    );
    ensure!(
        menu["betMultiplierDictatedByPaylines"] == false,
        "Microgaming paylines need a reviewed selector"
    );
    let coins = menu["minCoinsPerLineOrWay"]
        .as_u64()
        .filter(|v| *v > 0)
        .context("Microgaming minimum coins missing")?;
    let multipliers = menu["betMultipliers"]
        .as_array()
        .context("Microgaming multiplier menu missing")?;
    let sizes = menu["coinSizes"]
        .as_array()
        .context("Microgaming coin menu missing")?;
    for multiplier in multipliers {
        let multiplier = multiplier
            .as_u64()
            .filter(|v| *v > 0)
            .context("invalid Microgaming multiplier")?;
        for size in sizes {
            let size_amount = amount(size)?;
            if size_amount > Decimal::ZERO
                && exact_mul(
                    exact_mul(Decimal::from(coins), Decimal::from(multiplier))?,
                    size_amount,
                )? == minimum
            {
                return Ok((
                    json!({"betMultiplier":multiplier,"numCoinsPerLineOrWay":coins,"coinSize":size}),
                    minimum,
                ));
            }
        }
    }
    anyhow::bail!("Microgaming minimum has no matching wager parameters")
}

fn settlement(profile: &Profile, receipt: &Value) -> Result<Completion> {
    let context = &receipt["context"];
    let response = &receipt["gameResponse"];
    if response["summary"]["isComplete"] != true
        || response["command"]["name"] != "Spin"
        || context["isReadOnly"] != false
        || context["game"]["transactionNumber"] != 1
    {
        return Err(unknown_event(
            "Microgaming unrecognized spin closure/continuation",
        ));
    }
    Ok(Completion {
        native_round_id: None,
        details: json!({"module_id":MODULE,"transaction_number":1,"offered_bet":profile.raw_stake}),
    })
}

fn practice_request() -> Value {
    let attributes = [
        ("bn", ""),
        ("dos", "Windows 10"),
        ("dosv", "NT 10.0"),
        ("it", "false"),
        ("mktn", ""),
        ("mbb", "Firefox"),
        ("mbbv", "159.0"),
        ("mdln", ""),
        ("ua", USER_AGENT),
        ("ff", "desktop"),
    ]
    .into_iter()
    .map(|(key, value)| json!({"shortCode":key,"value":value}))
    .collect::<Vec<_>>();
    json!({"sessionProductId":PRODUCT,"numLaunchTokens":10,
        "environment":{"clientTypeId":40,"languageCode":"en"},"deviceAttributes":attributes,
        "context":{"moduleId":MODULE,"clientId":CLIENT,"intent":1}})
}

async fn post(
    ctx: &RunContext,
    label: &str,
    path: &str,
    payload: &Value,
    token: Option<&str>,
    round_action: bool,
) -> Result<Value> {
    let mut headers = BTreeMap::from([
        ("content-type".into(), "application/json".into()),
        ("accept".into(), "application/json".into()),
        ("accept-encoding".into(), "identity".into()),
        ("origin".into(), ORIGIN.into()),
        ("referer".into(), format!("{ORIGIN}/")),
        ("user-agent".into(), USER_AGENT.into()),
        ("x-clienttypeid".into(), "40".into()),
        ("x-correlationid".into(), Uuid::new_v4().to_string()),
        ("x-route-productid".into(), PRODUCT.to_string()),
        ("x-route-moduleid".into(), MODULE.to_string()),
    ]);
    if let Some(token) = token {
        headers.insert("authorization".into(), format!("Bearer {token}"));
    }
    let response = ctx
        .http(
            label,
            "POST",
            &format!("{API}{path}"),
            headers,
            serde_json::to_string(payload)?,
            round_action,
        )
        .await?;
    ensure!(
        response.status == 200,
        "Microgaming HTTP {} at {label}",
        response.status
    );
    serde_json::from_str(&response.body).context("Microgaming native JSON response invalid")
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    validate(game, mode, settings)?;
    let practice = post(
        ctx,
        "practice",
        "/user/public/v1/accounts/login/practice",
        &practice_request(),
        None,
        false,
    )
    .await?;
    let token = token(&practice)?;
    let path = format!("/play/public/v1/games2/module/{MODULE}/client/{CLIENT}/play/");
    let initial = post(
        ctx,
        "refresh",
        &format!("{path}refreshes"),
        &json!({"context":{"freeGames":null},"gameRequest":{}}),
        Some(token),
        false,
    )
    .await?;
    let profile = profile(&practice, &initial)?;
    ctx.save_session(&json!({"provider":"microgaming","game":GAME,"module_id":MODULE,"client_id":CLIENT,"product_id":PRODUCT,
        "user_token":profile.token,"practice":practice,"initialization":initial,
        "offered_bet":profile.raw_stake}))?;
    let receipt = post(
        ctx,
        "normal",
        &format!("{path}spins"),
        &json!({"context":{"freeGames":null},"gameRequest":{"bet":profile.raw_stake}}),
        Some(&profile.token),
        true,
    )
    .await?;
    // Only one normal action is demonstrated. An open round stays retained;
    // no guessed continuation and no replacement practice owner are created.
    settlement(&profile, &receipt)
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    game.id == GAME && mode.kind == ModeKind::Normal && mode.key == "normal"
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn fresh_profile_and_terminal_spin_do_not_need_money() {
        let initial = json!({"context":{"isReadOnly":true,"game":{"transactionNumber":-1}},"gameResponse":{"summary":{"isComplete":true},"financials":{"availableBetLimits":{"defaultBet":"0.1"}}}});
        let profile = profile(&json!({"tokens":{"userToken":"native-session"}}), &initial).unwrap();
        let receipt = json!({"context":{"isReadOnly":false,"game":{"transactionNumber":1}},"gameResponse":{"command":{"name":"Spin"},"summary":{"isComplete":true}}});
        assert!(settlement(&profile, &receipt).is_ok());
    }
    #[test]
    fn unknown_continuation_is_typed() {
        let profile = Profile {
            token: "s".into(),
            raw_stake: json!(1),
        };
        let err = settlement(
            &profile,
            &json!({"gameResponse":{"summary":{"isComplete":false}}}),
        )
        .unwrap_err();
        assert!(err.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
}
