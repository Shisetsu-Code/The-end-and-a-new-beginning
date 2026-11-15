//! Native capture protocol derived from standalone info/mascot/codigo and the evidence reference adapter.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
    time::Duration,
};
use url::Url;

#[derive(Deserialize)]
struct Binding {
    launcher: String,
    native_id: String,
    bet_coins: Option<i64>,
    #[serde(default)]
    family: Family,
    modes: BTreeMap<String, Entry>,
    continuations: Vec<Continuation>,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Family {
    #[default]
    Slot,
    Keno,
    Bingo,
    Roulette,
    Plinball,
    VideoPoker,
}

#[derive(Deserialize)]
struct Entry {
    entry: Value,
    purchase: Option<Value>,
    #[serde(default)]
    offer: bool,
}

#[derive(Deserialize)]
struct Continuation {
    feature: String,
    action: String,
    requests: Vec<Value>,
}

fn bindings() -> &'static BTreeMap<String, Binding> {
    static BINDINGS: OnceLock<BTreeMap<String, Binding>> = OnceLock::new();
    BINDINGS.get_or_init(|| {
        #[derive(Deserialize)]
        struct Extra {
            games: BTreeMap<String, Binding>,
        }
        let mut games: BTreeMap<String, Binding> =
            serde_json::from_slice(include_bytes!("../../bindings/mascot.json"))
                .expect("valid embedded Mascot bindings");
        let extra: Extra =
            serde_json::from_slice(include_bytes!("../../bindings/mascot-extra.json"))
                .expect("valid supplemental Mascot bindings");
        for (id, supplemental) in extra.games {
            if let Some(existing) = games.get_mut(&id) {
                assert!(
                    existing.launcher == supplemental.launcher
                        && existing.native_id == supplemental.native_id
                        && existing.bet_coins == supplemental.bet_coins
                        && existing.family == supplemental.family,
                    "supplemental Mascot identity must agree with existing evidence"
                );
                for (key, entry) in supplemental.modes {
                    assert!(
                        existing.modes.insert(key, entry).is_none(),
                        "supplemental Mascot modes must not replace original entries"
                    );
                }
                for next in supplemental.continuations {
                    if let Some(old) = existing
                        .continuations
                        .iter_mut()
                        .find(|old| old.feature == next.feature && old.action == next.action)
                    {
                        for request in next.requests {
                            if !old.requests.contains(&request) {
                                old.requests.push(request);
                            }
                        }
                    } else {
                        existing.continuations.push(next);
                    }
                }
            } else {
                games.insert(id, supplemental);
            }
        }
        games
    })
}

fn binding(game: &Game, mode: &Mode) -> Option<(&'static Binding, &'static Entry)> {
    let game = bindings().get(&game.id)?;
    let entry = game.modes.get(&mode.key)?;
    match (
        mode.kind,
        entry.entry["action"].as_str(),
        entry.entry["boost"].as_bool(),
    ) {
        (ModeKind::Normal, Some("spin"), Some(false))
        | (ModeKind::AnteBet, Some("spin"), Some(true))
            if game.family == Family::Slot =>
        {
            Some((game, entry))
        }
        (ModeKind::Purchase, Some("buyin"), _)
            if game.family == Family::Slot && (entry.purchase.is_some() || entry.offer) =>
        {
            Some((game, entry))
        }
        (ModeKind::Normal, Some("spin"), None)
            if game.family != Family::Slot && mode.key == "normal" =>
        {
            Some((game, entry))
        }
        _ => None,
    }
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    binding(game, mode).is_some()
}

fn closes(reply: &Value) -> bool {
    reply["feature"] == "basic" && reply["nextAction"] == "spin"
}

fn entry_bet(entry: &Entry) -> &Value {
    entry
        .entry
        .get("bet")
        .unwrap_or(&entry.entry["spin_bets"][0]["bet"])
}

fn validate_quote(init: &Value, binding: &Binding, entry: &Entry) -> Result<()> {
    ensure!(
        init["gameId"] == binding.native_id && closes(init),
        "Mascot did not initialize an idle demo"
    );
    let bet = entry_bet(entry);
    bet.as_i64()
        .filter(|v| *v > 0)
        .context("invalid bound Mascot bet")?;
    ensure!(
        init["bets"]
            .as_array()
            .is_some_and(|bets| bets.contains(bet)),
        "Mascot stake menu changed"
    );
    if entry.offer {
        let offer = init["buyin"]["offer"]
            .as_object()
            .context("Mascot dynamic offer not available in this fresh session")?;
        ensure!(
            offer.get("id") == entry.entry.get("buyinType"),
            "Mascot requested dynamic offer is not available"
        );
        if init["buyin"]["regular"].as_array().is_some_and(|offers| {
            offers
                .iter()
                .any(|offer| offer["id"] == entry.entry["buyinType"])
        }) {
            return Err(unknown_event(
                "Mascot duplicate regular/dynamic purchase selector",
            ));
        }
        // buyin.offer is recalculated by the native service. Its current id is
        // the control; captured prices and free-spin counts are not fixed quotes.
    }
    if let Some(expected) = &entry.purchase {
        let offers = init["buyin"]["regular"]
            .as_array()
            .context("Mascot purchase menu absent")?;
        let mut offers = offers
            .iter()
            .filter(|item| item["id"] == entry.entry["buyinType"]);
        let offer = offers.next().context("Mascot purchase unavailable")?;
        ensure!(
            offers.next().is_none()
                && ["type", "fs"]
                    .iter()
                    .all(|key| offer[*key] == expected[*key]),
            "Mascot purchase details changed"
        );
    }
    if entry.entry["boost"] == true {
        ensure!(
            init["buyin"]["boost"].is_object(),
            "Mascot antebet not offered"
        );
    }
    let shape_ok = match binding.family {
        Family::Slot => true,
        Family::Keno => entry.entry["nums"].as_array().is_some_and(|nums| {
            !nums.is_empty()
                && init["paytable"]
                    .as_array()
                    .is_some_and(|table| table.len() >= nums.len())
        }),
        Family::Bingo => entry.entry["cardIds"].as_array().is_some_and(|ids| {
            !ids.is_empty()
                && init["cards"].as_array().is_some_and(|cards| {
                    ids.iter()
                        .all(|id| id.as_u64().is_some_and(|id| id < cards.len() as u64))
                })
                && init["cardIds"]
                    .as_array()
                    .is_some_and(|offered| ids.iter().all(|id| offered.contains(id)))
        }),
        Family::Roulette => entry.entry["spin_bets"].as_array().is_some_and(|bets| {
            bets.len() == 1
                && bets[0]["type"] == "straightup"
                && bets[0]["position"] == 17
                && init["paytable"].get("straightup").is_some()
        }),
        Family::Plinball => {
            entry.entry["countBalls"] == 1
                && init["collection"].is_object()
                && init["paytable"].is_object()
        }
        Family::VideoPoker => {
            entry.entry["gameType"] == "JacksOrBetter"
                && entry.entry["handsCount"] == 1
                && init["paytable"]["paytable_JacksOrBetter"].is_array()
        }
    };
    if !shape_ok {
        return Err(unknown_event("Mascot native table controls changed"));
    }
    Ok(())
}

fn validate_reply(binding: &Binding, request: &Value, reply: &Value) -> Result<()> {
    ensure!(
        reply.get("error").is_none_or(Value::is_null),
        "Mascot rejected action"
    );
    if !(reply.is_object()
        && reply["action"] == request["action"]
        && reply["action"].is_string()
        && reply["feature"].is_string()
        && reply["nextAction"].is_string()
        && reply.get("gameId").is_none_or(|v| v == &binding.native_id))
    {
        return Err(unknown_event(
            "Mascot native response identity or control shape changed",
        ));
    }
    Ok(())
}

fn continuation(reply: &Value, binding: &Binding, bet: &Value) -> Result<Value> {
    let mut used = BTreeSet::new();
    if let Some(entries) = reply["bonus"]["items"].as_object() {
        used.extend(entries.keys().cloned());
    }
    if let Some(entries) = reply["bonus"]["lostItems"].as_object() {
        used.extend(entries.keys().cloned());
    }
    if let Some(entries) = reply["bonus"]["lostItems"].as_array() {
        used.extend(
            entries
                .iter()
                .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)),
        );
    }
    let mut request = binding
        .continuations
        .iter()
        .find(|next| reply["feature"] == next.feature && reply["nextAction"] == next.action)
        .and_then(|next| {
            next.requests.iter().find(|request| {
                request
                    .get("bonusItem")
                    .is_none_or(|id| !used.contains(&id.to_string()))
            })
        })
        .cloned()
        .ok_or_else(|| unknown_event("Mascot continuation requires a reviewed control"))?;
    if request.get("bet").is_some() {
        request["bet"] = bet.clone();
    }
    Ok(request)
}

async fn launch(ctx: &RunContext, launcher: &str) -> Result<Url> {
    let mut url = Url::parse("https://demo.mascot.games/")?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid Mascot launcher"))?
        .extend(["run", launcher]);
    for _ in 0..5 {
        let answer = ctx.get("demo_launch", url.as_str()).await?;
        if answer.status == 200 {
            ensure!(
                url.host_str() != Some("demo.mascot.games"),
                "Mascot demo did not issue a game host"
            );
            return url
                .join("/mascotGaming/spin.php")
                .context("invalid Mascot game endpoint");
        }
        ensure!(
            matches!(answer.status, 301 | 302 | 303 | 307 | 308),
            "Mascot unexpected launch status {}",
            answer.status
        );
        url = url.join(
            answer
                .headers
                .get("location")
                .context("Mascot redirect missing destination")?,
        )?;
    }
    bail!("Mascot demo redirect limit")
}

async fn send(ctx: &RunContext, endpoint: &Url, request: &Value, financial: bool) -> Result<Value> {
    let origin = endpoint.origin().ascii_serialization();
    let action = request["action"].as_str().context("Mascot action absent")?;
    tokio::time::sleep(Duration::from_millis(if action == "collect" {
        2600
    } else {
        1000
    }))
    .await;
    ctx.json(
        action,
        endpoint.as_str(),
        request,
        BTreeMap::from([
            ("origin".into(), origin.clone()),
            ("referer".into(), format!("{origin}/")),
        ]),
        financial,
    )
    .await
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &serde_json::Value,
) -> Result<Completion> {
    let (binding, profile) = binding(game, mode).context("Mascot mode has no native binding")?;
    let endpoint = launch(ctx, &binding.launcher).await?;
    let init = send(ctx, &endpoint, &json!({"action":"init"}), false).await?;
    validate_quote(&init, binding, profile)?;
    ctx.save_session(&json!({"game":game.id,"endpoint":endpoint.as_str(),"currency":"NAN"}))?;
    let first = send(ctx, &endpoint, &profile.entry, true).await?;
    validate_reply(binding, &profile.entry, &first)?;
    let mut responses = vec![first];
    for _ in 0..ctx.max_steps().min(300) {
        let last = responses.last().context("Mascot response missing")?;
        if closes(last) {
            return Ok(Completion {
                native_round_id: None,
                details: json!({"steps":responses.len(),"mode":mode.key}),
            });
        }
        if responses.len() >= ctx.max_steps().min(300) {
            break;
        }
        let request = continuation(last, binding, entry_bet(profile))?;
        let reply = send(ctx, &endpoint, &request, true).await?;
        validate_reply(binding, &request, &reply)?;
        responses.push(reply);
    }
    bail!("Mascot continuation limit reached")
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn supplemental_native_entries_and_recorded_chains_are_supported() {
        let extra: Value =
            serde_json::from_slice(include_bytes!("../../bindings/mascot-extra.json")).unwrap();
        let fixtures = extra["fixtures"].as_array().unwrap();
        assert_eq!(fixtures.len(), 31);
        for fixture in fixtures {
            let game: Game = serde_json::from_value(json!({
                "id": fixture["game"], "modes": []
            }))
            .unwrap();
            let key = fixture["mode"].as_str().unwrap();
            let mode: Mode = serde_json::from_value(json!({
                "key": key, "type": if key == "normal" { "normal" } else { "purchase" }
            }))
            .unwrap();
            let (bound, entry) =
                binding(&game, &mode).unwrap_or_else(|| panic!("missing {}/{}", game.id, mode.key));
            validate_quote(&fixture["init"], bound, entry)
                .unwrap_or_else(|e| panic!("invalid fresh quote {}/{}: {e}", game.id, mode.key));
            let steps = fixture["steps"].as_array().unwrap();
            assert_eq!(steps[0]["request"], entry.entry);
            for step in steps {
                validate_reply(bound, &step["request"], &step["response"]).unwrap_or_else(|e| {
                    panic!("invalid native response {}/{}: {e}", game.id, mode.key)
                });
                assert!(step["response"].get("balance").is_none());
                assert!(step["response"].get("bet").is_none());
            }
            for pair in steps.windows(2) {
                let request = continuation(&pair[0]["response"], bound, entry_bet(entry))
                    .unwrap_or_else(|e| {
                        panic!("unknown native continuation {}/{}: {e}", game.id, mode.key)
                    });
                assert_eq!(request["action"], pair[1]["request"]["action"]);
                assert!(
                    bound
                        .continuations
                        .iter()
                        .any(|next| next.feature == pair[0]["response"]["feature"]
                            && next.action == pair[0]["response"]["nextAction"]
                            && next.requests.contains(&pair[1]["request"])),
                    "captured request must have a binding for its preceding native state"
                );
            }
            assert!(closes(&steps.last().unwrap()["response"]));
        }
        assert_eq!(bindings().len(), 126);
        assert_eq!(
            bindings().values().map(|b| b.modes.len()).sum::<usize>(),
            211
        );
    }

    #[test]
    fn dynamic_offer_uses_fresh_selector_without_freezing_historic_price() {
        let extra: Value =
            serde_json::from_slice(include_bytes!("../../bindings/mascot-extra.json")).unwrap();
        let fixture = &extra["fixtures"][0];
        let bound = &bindings()[fixture["game"].as_str().unwrap()];
        let entry = &bound.modes[fixture["mode"].as_str().unwrap()];
        assert!(entry.offer);
        let mut init = fixture["init"].clone();
        init["buyin"]["offer"]["fs"] = json!("30");
        init["buyin"]["offer"]["bet"] = json!(1234);
        init.as_object_mut().unwrap().remove("betCoins");
        assert!(validate_quote(&init, bound, entry).is_ok());
        init["buyin"]["regular"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":entry.entry["buyinType"]}));
        let error = validate_quote(&init, bound, entry).unwrap_err();
        assert!(error.downcast_ref::<crate::model::UnknownEvent>().is_some());
        init["buyin"]["regular"].as_array_mut().unwrap().pop();
        init["buyin"]["offer"]["id"] = json!(99);
        assert!(validate_quote(&init, bound, entry).is_err());
        init["buyin"].as_object_mut().unwrap().remove("offer");
        assert!(validate_quote(&init, bound, entry).is_err());
    }

    #[test]
    fn changed_table_controls_and_unrelated_receipts_stop_with_unknown_event() {
        let extra: Value =
            serde_json::from_slice(include_bytes!("../../bindings/mascot-extra.json")).unwrap();
        for fixture in extra["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["mode"] == "normal")
        {
            let bound = &bindings()[fixture["game"].as_str().unwrap()];
            let entry = &bound.modes["normal"];
            let mut init = fixture["init"].clone();
            match bound.family {
                Family::Bingo => {
                    init["cards"].as_array_mut().unwrap().pop();
                }
                Family::Plinball => {
                    init.as_object_mut().unwrap().remove("collection");
                }
                _ => {
                    init.as_object_mut().unwrap().remove("paytable");
                }
            }
            assert!(
                validate_quote(&init, bound, entry)
                    .unwrap_err()
                    .downcast_ref::<crate::model::UnknownEvent>()
                    .is_some()
            );
            let mut reply = fixture["steps"][0]["response"].clone();
            reply["action"] = json!("unrecognized");
            assert!(
                validate_reply(bound, &entry.entry, &reply)
                    .unwrap_err()
                    .downcast_ref::<crate::model::UnknownEvent>()
                    .is_some()
            );
            reply["action"] = entry.entry["action"].clone();
            reply["gameId"] = json!("different_game");
            assert!(validate_reply(bound, &entry.entry, &reply).is_err());
        }
    }

    #[test]
    fn bonus_choices_skip_lost_items_reported_as_objects() {
        let bound: Binding = serde_json::from_value(json!({
            "launcher":"example", "native_id":"example", "bet_coins":1,
            "modes":{}, "continuations":[{"feature":"bonus", "action":"bonus", "requests":[
                {"action":"bonus", "bonusItem":1}, {"action":"bonus", "bonusItem":2}
            ]}]
        }))
        .unwrap();
        let request = continuation(
            &json!({
                "feature":"bonus", "nextAction":"bonus",
                "bonus":{"items":{"0":{}}, "lostItems":{"1":{}}}
            }),
            &bound,
            &json!(5),
        )
        .unwrap();
        assert_eq!(request, json!({"action":"bonus", "bonusItem":2}));
    }

    #[test]
    fn closure_without_wallet() {
        assert!(closes(&json!({"feature":"basic","nextAction":"spin"})));
    }
    #[test]
    fn unknown_feature_does_not_invent_action() {
        let binding: Binding = serde_json::from_value(
            json!({"launcher":"test","native_id":"t","bet_coins":1,"modes":{},"continuations":[]}),
        )
        .unwrap();
        let err = continuation(
            &json!({"feature":"new","nextAction":"invented"}),
            &binding,
            &json!(1),
        )
        .unwrap_err();
        assert!(err.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
}
