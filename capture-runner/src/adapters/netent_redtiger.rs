//! Native capture protocol from standalone info/netent/codigo/netent_redtiger.rs.
//! Published evidence SHA-256: 99759470110eca3a3182d5937e0ad1bc7a22fe8c106648cbdc2ca1068bca6e3e
//! Reference transport game-runner/src/adapters/netent_redtiger.rs; no runtime dependency.
use super::netent_launch::open_config;
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal},
};
use anyhow::{Context, Result, ensure};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::LazyLock};
use url::Url;
use uuid::Uuid;

const VERSION: &str = "6.20261030.73038.67000-7e7b03116d-r2";

#[derive(Deserialize)]
struct Profile {
    native_table: String,
    native_game: String,
    modes: BTreeMap<String, Selector>,
}

#[derive(Deserialize)]
struct Selector {
    id: u64,
    kind: String,
}

static PROFILES: LazyLock<BTreeMap<String, Profile>> = LazyLock::new(|| {
    serde_json::from_slice(include_bytes!("../../bindings/netent-redtiger.json"))
        .expect("compiled NetEnt Red Tiger bindings")
});

fn profile(game: &Game, mode: &Mode) -> Option<(&'static Profile, &'static Selector)> {
    let profile = PROFILES.get(&game.id)?;
    let selected = profile.modes.get(&mode.key)?;
    match (selected.kind.as_str(), selected.id, mode.kind) {
        ("normal", 0, ModeKind::Normal)
        | ("featureBuy", 1.., ModeKind::Purchase | ModeKind::Variant) => Some((profile, selected)),
        _ => None,
    }
}

pub(super) fn supports(game: &Game, mode: &Mode) -> bool {
    profile(game, mode).is_some()
}

fn pending_choice(value: &Value) -> bool {
    match value {
        Value::Object(fields) => {
            fields
                .get("choices")
                .and_then(|v| v.get("available"))
                .is_some_and(|choices| {
                    !choices.is_null()
                        && !choices.as_array().is_some_and(Vec::is_empty)
                        && !choices.as_object().is_some_and(|v| v.is_empty())
                })
                || fields.values().any(pending_choice)
        }
        Value::Array(values) => values.iter().any(pending_choice),
        _ => false,
    }
}

fn atomic_closed(config: &Value, result: &Value) -> bool {
    let game = &result["game"];
    let persistent = config["stateful"] == true
        && config["hasGambleGame"] != true
        && matches!(
            game["spinMode"].as_str(),
            Some("Normal" | "FreeSpins" | "FreeSpin" | "Respins" | "Respin")
        );
    !pending_choice(result)
        && (config["hasChoices"] != true
            || matches!(result["bonusChance"], Value::Null | Value::Bool(false)))
        && matches!(game["pending"], Value::Null | Value::Bool(false))
        && (game["nextAction"].is_null() || game["nextAction"] == "spin")
        && (game["hasState"] == false || game["hasState"] == true && persistent)
}

pub(super) async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    let (profile, selected) =
        profile(game, mode).context("unreviewed NetEnt Red Tiger selector")?;
    let launch = open_config(
        ctx,
        &game.id,
        &profile.native_table,
        "redtiger_http",
        VERSION,
    )
    .await?;
    let config = &launch.config;
    let wrapped = &config["wrappedLaunchParams"];
    ensure!(
        config["table_id"] == profile.native_table
            && config["game_provider"] == "extendednetent"
            && config["base_game_id"] == profile.native_game
            && config["userType"] == 0
            && wrapped["gameId"] == profile.native_game,
        "NetEnt Red Tiger demo identity changed"
    );
    let base = Url::parse(text(wrapped, "baseUrl")?)?;
    ensure!(
        base.scheme() == "https"
            && base.port().is_none()
            && base.username().is_empty()
            && base.password().is_none()
            && base.fragment().is_none()
            && base.query().is_none(),
        "invalid NetEnt Red Tiger service URL"
    );
    let platform = format!("{}/platform/game", base.as_str().trim_end_matches('/'));
    let headers = BTreeMap::from([
        ("accept".into(), "*/*".into()),
        ("origin".into(), base.origin().ascii_serialization()),
    ]);
    let mut params = json!({
        "token": null, "sessionId": text(wrapped, "token")?, "playMode": "demo",
        "gameId": profile.native_game, "listenToFrontend": true,
        "custom": {"siteId": wrapped["casino"], "context": {"ne_evo_token": text(config, "wrapper_token")?}},
        "userData": {"userId": "demo-user", "hash": "", "affiliate": "", "lang": text(wrapped, "lang")?,
            "channel": text(wrapped, "channel")?, "userType": "U", "fingerprint": Uuid::new_v4().to_string()},
    });
    let settings = ctx
        .json(
            "netent_redtiger_settings",
            &format!("{platform}/settings"),
            &params,
            headers.clone(),
            false,
        )
        .await?;
    ensure!(
        settings["success"] == true,
        "NetEnt Red Tiger demo settings declined"
    );
    let configured = &settings["result"];
    let user = &configured["user"];
    let menu = user["stakes"]["types"]
        .as_array()
        .context("native stake menu absent")?;
    let stake = menu
        .iter()
        .map(money)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|value| *value > Decimal::ZERO)
        .min()
        .context("no positive stake offered")?;
    let expected_kind = if selected.id == 0 {
        ModeKind::Normal
    } else {
        ModeKind::Purchase
    };
    params["token"] = json!(text(user, "token")?);
    params["sessionId"] = json!(text(user, "sessionId")?);
    let owner = &user["userId"];
    ensure!(
        owner.is_number() || owner.as_str().is_some_and(|id| !id.is_empty()),
        "demo owner missing"
    );
    params["userData"]["userId"] = owner.clone();
    ctx.save_session(&params)?;
    params["stake"] = serde_json::from_str(&stake.to_string())?;
    params["bonusId"] = Value::Null;
    params["gameMode"] = json!(selected.id);
    let spin = ctx
        .json(
            "netent_redtiger_spin",
            &format!("{platform}/spin"),
            &params,
            headers,
            true,
        )
        .await?;
    ensure!(spin["success"] == true, "NetEnt Red Tiger spin declined");
    let result = &spin["result"];
    if selected.id != 0 {
        let (selected_mode, _) = mode
            .key
            .split_once(':')
            .context("invalid reviewed bonus label")?;
        ensure!(
            result["game"]["spinMode"] == selected_mode,
            "NetEnt Red Tiger bonus response mismatch"
        );
    }
    if !(atomic_closed(&configured["game"], result)) {
        return Err(crate::model::unknown_event(
            "NetEnt Red Tiger round has an unreviewed continuation".to_string(),
        ));
    };
    let round = &result["transactions"]["roundId"];
    ensure!(
        round.is_string() || round.is_number(),
        "native round id missing"
    );
    Ok(Completion {
        native_round_id: Some(
            round
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| round.to_string()),
        ),
        details: json!({"table":profile.native_table,"game":profile.native_game,"native_mode":selected.id,"spin_mode":result["game"]["spinMode"],"native_operation":expected_kind.operation()}),
    })
}

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("Netent Red Tiger missing {key}"))
}
fn money(v: &Value) -> Result<Decimal> {
    decimal(
        &v.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string()),
    )
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn native_atomic_closure_needs_no_transactions_or_wallet() {
        assert!(atomic_closed(
            &json!({"stateful":false}),
            &json!({"game":{"hasState":false,"nextAction":"spin","pending":false}})
        ));
        assert!(!atomic_closed(
            &json!({"stateful":false}),
            &json!({"game":{"hasState":true,"nextAction":"novel"}})
        ));
        assert!(!atomic_closed(
            &Value::Null,
            &json!({"game":{"hasState":false,"choices":{"available":["pick"]}}})
        ));
    }
}
