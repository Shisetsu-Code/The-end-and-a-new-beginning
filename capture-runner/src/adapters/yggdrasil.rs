//! Native protocol reconstructed from standalone info/yggdrasil/codigo; no old runtime dependency.
//! Native selectors from every published CSV row and the supplied handoff protocol.
//! Historical rejection/closure is metadata; current offers determine dispatch.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock};
use url::Url;
use uuid::Uuid;

const SERVICE: &str = "https://demo.yggdrasilgaming.com/game.web/service";
const ORIGIN: &str = "https://staticdemo.yggdrasilgaming.com";
#[derive(Deserialize)]
struct Binding {
    kind: ModeKind,
    coin: String,
    amount: String,
    command: String,
    source_kind: String,
}
#[derive(Deserialize)]
struct Bindings {
    games: BTreeMap<String, BTreeMap<String, Binding>>,
}
fn bindings() -> &'static Bindings {
    static BINDINGS: OnceLock<Bindings> = OnceLock::new();
    BINDINGS.get_or_init(|| {
        serde_json::from_str(include_str!("../../bindings/yggdrasil.json"))
            .expect("embedded Yggdrasil entry bindings")
    })
}
fn binding(game: &Game, mode: &Mode) -> Option<&'static Binding> {
    let row = bindings().games.get(&game.id)?.get(&mode.key)?;
    (row.kind == mode.kind && row.command == mode.key).then_some(row)
}
pub fn validate(game: &Game, mode: &Mode, settings: &Value) -> Result<()> {
    let row = binding(game, mode)
        .ok_or_else(|| unknown_event("Yggdrasil native selector is not source-bound"))?;
    ensure!(
        decimal(&row.coin)? > Decimal::ZERO && decimal(&row.amount)? > Decimal::ZERO,
        "Yggdrasil binding has a nonpositive stake"
    );
    if !settings.is_null() {
        let overrides = settings
            .as_object()
            .context("Yggdrasil settings must be an object")?;
        for (key, value) in overrides {
            let expected = match key.as_str() {
                "coin" => &row.coin,
                "amount" | "stake" => &row.amount,
                _ => bail!("Yggdrasil unknown entry override: {key}"),
            };
            ensure!(
                amount(value)? == decimal(expected)?,
                "Yggdrasil override differs from source-bound selector: {key}"
            );
        }
    }
    Ok(())
}
fn amount(value: &Value) -> Result<Decimal> {
    let parsed = match value {
        Value::String(s) => decimal(s),
        Value::Number(n) => decimal(&n.to_string()),
        _ => return Err(unknown_event("native Yggdrasil amount missing")),
    };
    parsed.map_err(|_| unknown_event("native Yggdrasil amount format changed"))
}
fn common(game: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("gameid".into(), game.into()),
        ("channel".into(), "pc".into()),
        ("currency".into(), "EUR".into()),
        ("lang".into(), "en".into()),
    ])
}

fn current_stake(config: &Value, row: &Binding) -> Result<()> {
    let modes = config["bets"]["betModes"]
        .as_array()
        .ok_or_else(|| unknown_event("native Yggdrasil bet modes missing"))?;
    let offered: Vec<_> = modes.iter().filter(|m| m["name"] == row.command).collect();
    if offered.len() != 1 {
        return Err(unknown_event(
            "native Yggdrasil requested mode missing or ambiguous",
        ));
    }
    let selected = offered[0];
    let types = selected.get("modeTypes").and_then(Value::as_array);
    if row.source_kind == "normal"
        && types.is_some_and(|types| types.iter().any(|t| t == "BUY_BONUS" || t == "GOLDEN_BET"))
    {
        return Err(unknown_event("native Yggdrasil BASIC changed role"));
    }
    let parsed;
    let bets = if let Some(text) = selected["bets"].as_str() {
        parsed = serde_json::from_str::<Value>(text)?;
        &parsed
    } else {
        &selected["bets"]
    };
    let bets = bets
        .as_array()
        .ok_or_else(|| unknown_event("native Yggdrasil bet list missing"))?;
    let mut found = false;
    for bet in bets {
        if bet["available"] == true
            && amount(&bet["coinValue"])? == decimal(&row.coin)?
            && amount(&bet["betAmount"])? == decimal(&row.amount)?
        {
            found = true;
        }
    }
    ensure!(found, "published Yggdrasil requested stake unavailable");
    Ok(())
}
fn wager<'a>(data: &'a Value, id: &Value, game: &str) -> Result<&'a Value> {
    let wager = &data["wager"];
    if &wager["wagerid"] != id {
        return Err(unknown_event("native Yggdrasil wager changed"));
    }
    if !(id.is_number() || id.as_str().is_some_and(|s| !s.is_empty())) {
        return Err(unknown_event("native Yggdrasil wager identity missing"));
    }
    let _bets = wager["bets"]
        .as_array()
        .filter(|b| !b.is_empty())
        .ok_or_else(|| unknown_event("native Yggdrasil bets missing"))?;
    if let Some(actual) = wager.get("gameid")
        && !(actual.as_str() == Some(game)
            || actual.as_u64().is_some_and(|id| id.to_string() == game))
    {
        return Err(unknown_event("native Yggdrasil game changed"));
    }

    Ok(wager)
}
fn continuation(
    data: &Value,
    id: &Value,
    previous: Option<u64>,
    game: &str,
) -> Result<Option<(u64, String)>> {
    let wager = wager(data, id, game)?;
    if wager["status"] == "Finished" {
        return Ok(None);
    }
    if wager["status"] != "Pending" {
        return Err(unknown_event(
            "native Yggdrasil wager state is not recognized",
        ));
    }
    let last = wager["bets"].as_array().unwrap().last().unwrap();
    let step = last["step"]
        .as_u64()
        .ok_or_else(|| unknown_event("native Yggdrasil continuation step missing"))?;
    if previous.is_some_and(|old| step <= old) {
        return Err(unknown_event("native Yggdrasil step did not advance"));
    }
    let commands = last["betdata"]["cmds"]
        .as_array()
        .filter(|c| !c.is_empty())
        .ok_or_else(|| unknown_event("Yggdrasil continuation not offered"))?;
    let command = commands[0]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| unknown_event("native Yggdrasil continuation command invalid"))?;
    if commands.len() != 1 || command != "freeSpin" {
        return Err(unknown_event(format!(
            "Yggdrasil unsupported native commands: {commands:?}"
        )));
    }
    Ok(Some((step, command.to_owned())))
}

async fn request(
    ctx: &RunContext,
    method: &str,
    function: &str,
    fields: &BTreeMap<String, String>,
    token: Option<&str>,
    round_action: bool,
) -> Result<Value> {
    if !cfg!(test) {
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    }
    let mut endpoint = Url::parse(SERVICE)?;
    endpoint.query_pairs_mut().append_pair("fn", function);
    let mut headers = BTreeMap::from([
        ("origin".into(), ORIGIN.into()),
        ("referer".into(), format!("{ORIGIN}/")),
    ]);
    if let Some(token) = token {
        headers.insert("authorization".into(), format!("Bearer {token}"));
        headers.insert(
            "game-id".into(),
            fields
                .get("gameid")
                .context("Yggdrasil request game missing")?
                .clone(),
        );
    }
    if round_action {
        headers.insert("x-crid".into(), Uuid::new_v4().to_string());
    }
    let body = if method == "GET" {
        endpoint.query_pairs_mut().extend_pairs(fields);
        String::new()
    } else {
        headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish()
    };
    let response = ctx
        .http(
            &format!("yggdrasil.{function}"),
            method,
            endpoint.as_str(),
            headers,
            body,
            round_action,
        )
        .await?;
    ensure!(response.status == 200, "Yggdrasil HTTP {}", response.status);
    let reply: Value =
        serde_json::from_str(&response.body).context("invalid Yggdrasil native JSON")?;
    let code = reply["code"]
        .as_i64()
        .ok_or_else(|| unknown_event("Yggdrasil response status format changed"))?;
    ensure!(code == 0, "Yggdrasil native command rejected: {code}");
    reply
        .get("data")
        .cloned()
        .filter(|d| d.is_object())
        .ok_or_else(|| unknown_event("native Yggdrasil data missing"))
}

fn entry_fields(game: &str, row: &Binding) -> BTreeMap<String, String> {
    let mut entry = common(game);
    entry.extend([
        ("channelID".into(), String::new()),
        ("channelSuffix".into(), String::new()),
        ("gameHistorySessionId".into(), "session".into()),
        ("gameHistoryTicketId".into(), "ticket".into()),
        ("amount".into(), row.amount.clone()),
        ("coin".into(), row.coin.clone()),
        ("cmd".into(), row.command.clone()),
    ]);
    entry
}
fn continuation_fields(
    game: &str,
    id: &Value,
    step: u64,
    command: String,
) -> Result<BTreeMap<String, String>> {
    let mut follow = common(game);
    follow.extend([
        ("channelID".into(), String::new()),
        ("channelSuffix".into(), String::new()),
        ("gameHistorySessionId".into(), "session".into()),
        ("gameHistoryTicketId".into(), "ticket".into()),
        (
            "step".into(),
            step.checked_add(1)
                .context("native Yggdrasil step overflow")?
                .to_string(),
        ),
        ("betid".into(), "1".into()),
        ("cmd".into(), command),
        (
            "wagerid".into(),
            id.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| id.to_string()),
        ),
        ("amount".into(), "0.00".into()),
    ]);
    Ok(follow)
}
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    validate(game, mode, settings)?;
    let row = binding(game, mode).context("Yggdrasil requested binding missing")?;
    let limit = ctx.max_steps();
    ensure!(limit > 0, "Yggdrasil native action budget is empty");
    let mut demo = common(&game.id);
    demo.insert("org".into(), "Demo".into());
    demo.insert("license".into(), "mt".into());
    let config = request(ctx, "GET", "game", &demo, None, false).await?;
    demo.insert("userName".into(), "undefined".into());
    let auth = request(ctx, "POST", "authenticate", &demo, None, false).await?;
    current_stake(&config, row)?;
    let token = auth["token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("fresh Yggdrasil demo token missing")?;
    ctx.save_session(&json!({"provider":"yggdrasil","game":game.id,"org":"Demo","service":SERVICE,"token":token,"currency":"EUR"}))?;
    let entry = entry_fields(&game.id, row);
    let mut data = request(ctx, "POST", "play", &entry, Some(token), true).await?;
    let id = data["wager"]["wagerid"].clone();
    let mut previous = None;
    for index in 0..limit {
        match continuation(&data, &id, previous, &game.id)? {
            None => {
                let round_id = id
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| id.to_string());
                return Ok(Completion {
                    native_round_id: Some(round_id),
                    details: json!({"game":game.id,"steps":index+1,"status":"Finished"}),
                });
            }
            Some((step, command)) => {
                // The paid root already consumed one action. Always inspect
                // the current response before checking whether another send
                // fits: a terminal response at the limit is still verified.
                ensure!(
                    index + 1 < limit,
                    "Yggdrasil continuation limit; original wager retained"
                );
                previous = Some(step);
                let follow = continuation_fields(&game.id, &id, step, command)?;
                data = request(ctx, "POST", "cont", &follow, Some(token), true).await?;
            }
        }
    }
    bail!("Yggdrasil continuation limit; original wager retained")
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    binding(game, mode).is_some()
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn finished_native_wager_has_no_balance_dependency() {
        assert!(
            continuation(
                &json!({"wager":{"wagerid":"r","status":"Finished","bets":[{}]}}),
                &json!("r"),
                None,
                "7301"
            )
            .unwrap()
            .is_none()
        );
    }
    #[test]
    fn only_proven_native_command_is_replayed() {
        let packet = |command: &str| json!({"wager":{"wagerid":"r","status":"Pending","bets":[{"step":0,"betdata":{"cmds":[command]}}]}});
        assert_eq!(
            continuation(&packet("freeSpin"), &json!("r"), None, "7301").unwrap(),
            Some((0, "freeSpin".into()))
        );
        assert!(
            continuation(&packet("new_control"), &json!("r"), None, "7301")
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}

#[cfg(test)]
mod expanded_binding_tests {
    use super::*;
    #[test]
    fn all_published_selectors_are_supported_without_a_historical_success_gate() {
        let inventory: crate::model::Provider = serde_json::from_str(include_str!(
            "../../../standalone info/yggdrasil/inventario.json"
        ))
        .unwrap();
        let supported: usize = inventory
            .games
            .iter()
            .map(|game| {
                game.modes
                    .iter()
                    .filter(|mode| supports(game, mode))
                    .count()
            })
            .sum();
        assert_eq!(supported, 301);
        for (id, key) in [
            ("7304", "BASIC"),
            ("7400", "BUY_RAGNAROK"),
            ("7406", "GOLDEN_BET"),
        ] {
            let game = inventory.games.iter().find(|g| g.id == id).unwrap();
            let mode = game.modes.iter().find(|m| m.key == key).unwrap();
            assert!(supports(game, mode), "{id}/{key}");
        }
        let game = inventory.games.iter().find(|g| g.id == "7325").unwrap();
        assert!(supports(game, &game.modes[0]));
        let mut unknown = game.modes[0].clone();
        unknown.key = "NOT_PUBLISHED".into();
        assert!(!supports(game, &unknown));
    }
}

#[cfg(test)]
mod selector_tests {
    use super::*;
    #[test]
    fn every_bound_mode_uses_its_exact_offer_and_paid_command() {
        let inventory: crate::model::Provider = serde_json::from_str(include_str!(
            "../../../standalone info/yggdrasil/inventario.json"
        ))
        .unwrap();
        for game in &inventory.games {
            for mode in &game.modes {
                let Some(row) = binding(game, mode) else {
                    continue;
                };
                let offered = json!({"bets":{"betModes":[{"name":mode.key,"bets":[{"available":true,"coinValue":row.coin,"betAmount":row.amount}]}]}});
                current_stake(&offered, row).unwrap();
                let fields = entry_fields(&game.id, row);
                assert_eq!(fields["cmd"], mode.key);
                assert_eq!(fields["gameid"], game.id);
                assert_eq!(fields["coin"], row.coin);
                assert_eq!(fields["amount"], row.amount);
                let mut wrong = mode.clone();
                wrong.kind = if mode.kind == ModeKind::Normal {
                    ModeKind::Purchase
                } else {
                    ModeKind::Normal
                };
                assert!(!supports(game, &wrong));
            }
        }
        let game = &inventory.games.iter().find(|g| g.id == "7400").unwrap();
        let mode = game.modes.iter().find(|m| m.key == "BUY_RAGNAROK").unwrap();
        let row = binding(game, mode).unwrap();
        assert_eq!(entry_fields(&game.id, row)["amount"], "82.5");
        assert!(validate(game, mode, &json!({"coin":"0.1","amount":"1","stake":"1"})).is_err());
        validate(
            game,
            mode,
            &json!({"coin":"0.01","amount":"82.5","stake":"82.5"}),
        )
        .unwrap();
        assert!(
            current_stake(
                &json!({"bets":{"betModes":[{"name":"BASIC","bets":[]} ]}}),
                row
            )
            .unwrap_err()
            .downcast_ref::<crate::model::UnknownEvent>()
            .is_some()
        );
    }
    #[test]
    fn continuation_preserves_owner_and_only_offered_free_control() {
        let data = json!({"wager":{"gameid":7400,"wagerid":"owner","status":"Pending","bets":[{"step":7,"betdata":{"cmds":["freeSpin"]}}]}});
        let (step, command) = continuation(&data, &json!("owner"), Some(6), "7400")
            .unwrap()
            .unwrap();
        let fields = continuation_fields("7400", &json!("owner"), step, command).unwrap();
        assert_eq!(fields["step"], "8");
        assert_eq!(fields["betid"], "1");
        assert_eq!(fields["wagerid"], "owner");
        assert_eq!(fields["gameid"], "7400");
        assert_eq!(fields["cmd"], "freeSpin");
        assert_eq!(fields["amount"], "0.00");
        assert!(
            continuation(&data, &json!("owner"), None, "7301")
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
        assert!(
            continuation(&data, &json!("different-owner"), None, "7400")
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
        for (field, value) in [
            ("status", json!("NewPhase")),
            (
                "bets",
                json!([{"step":8,"betdata":{"cmds":["freeSpin","pick"]}}]),
            ),
        ] {
            let mut changed = data.clone();
            changed["wager"][field] = value;
            assert!(
                continuation(&changed, &json!("owner"), None, "7400")
                    .unwrap_err()
                    .downcast_ref::<crate::model::UnknownEvent>()
                    .is_some()
            );
        }
    }
}

#[cfg(test)]
mod source_table_tests {
    use super::*;
    #[test]
    fn embedded_entries_match_every_csv_selector_after_exact_deduplication() {
        let source = include_str!(
            "../../../standalone info/yggdrasil/referencias/provider-work/yggdrasil-export/yggdrasil-rounds.csv"
        );
        let mut unique = std::collections::BTreeSet::new();
        let mut rows = 0;
        for line in source.lines().skip(1) {
            // This fixed published table contains no quoted/comma-bearing titles.
            // Reject a layout change rather than silently changing column identity.
            let cells: Vec<_> = line.split(',').collect();
            assert_eq!(cells.len(), 18);
            rows += 1;
            let row = &bindings().games[cells[0]][cells[3]];
            assert_eq!(row.command, cells[3]);
            assert_eq!(row.coin, cells[4]);
            assert_eq!(row.amount, cells[5]);
            if !cells[6].is_empty() {
                assert_eq!(cells[5], cells[6]);
            }
            unique.insert((cells[0], cells[3]));
        }
        assert_eq!(rows, 305);
        assert_eq!(unique.len(), 301);
        assert_eq!(bindings().games.len(), 138);
        assert_eq!(
            bindings().games.values().map(BTreeMap::len).sum::<usize>(),
            unique.len()
        );
    }
}
