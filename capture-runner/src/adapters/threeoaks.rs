//! 3 Oaks demo command protocol reconstructed from public client serializer evidence.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, now_ms, sha256, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use uuid::Uuid;
const SITE: &str = "https://3oaks.com";
fn serializers() -> Value {
    serde_json::from_str(include_str!("../../bindings/threeoaks-serializers.json"))
        .expect("checked public serializers")
}
fn continuations() -> Value {
    serde_json::from_str(include_str!("../../bindings/threeoaks-continuations.json"))
        .expect("checked public continuations")
}
fn native_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
pub fn supports(game: &Game, mode: &Mode) -> bool {
    if !native_id(&game.id) {
        return false;
    }
    if mode.kind == ModeKind::Normal && mode.key == "normal" {
        return true;
    }
    let Some((prefix, id)) = mode.key.split_once('-') else {
        return false;
    };
    if id.is_empty() {
        return false;
    }
    if !matches!(
        (prefix, mode.kind),
        ("buy", ModeKind::Purchase) | ("booster", ModeKind::Booster) | ("ante", ModeKind::AnteBet)
    ) {
        return false;
    }
    serializers().as_object().is_some_and(|map| {
        map.values().any(|b| {
            b["game_id"] == game.id
                && if prefix == "buy" {
                    b["buy_schema"]
                        .as_str()
                        .is_some_and(|v| !matches!(v, "none" | "pending"))
                } else {
                    b["booster_schema"]
                        .as_str()
                        .is_some_and(|v| !matches!(v, "none" | "pending"))
                }
        })
    })
}
fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}
fn has(v: &Value, name: &str) -> bool {
    array(v).iter().any(|v| v == name)
}
fn key(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string())
}
fn native_int(v: &Value) -> Result<i64> {
    if let Some(i) = v.as_i64() {
        return Ok(i);
    }
    let text = key(v);
    let whole = match text.split_once('.') {
        Some((whole, fraction)) if fraction.bytes().all(|c| c == b'0') => whole,
        _ => &text,
    };
    whole
        .parse()
        .context("3 Oaks native selector must be an integer")
}
fn factor(start: &Value, lines: i64) -> Result<i64> {
    let settings = &start["settings"];
    let index = array(&settings["lines"])
        .iter()
        .position(|v| native_int(v).ok() == Some(lines))
        .context("3 Oaks selected lines not offered")?;
    let factors = array(&settings["bet_factor"]);
    let f = if factors.len() == 1 {
        &factors[0]
    } else if factors.len() == array(&settings["lines"]).len() {
        &factors[index]
    } else {
        return Err(unknown_event("3 Oaks line factor schema changed"));
    };
    native_int(f)
}
fn entry(start: &Value, binding: &Value, selected: &str) -> Result<Value> {
    let settings = &start["settings"];
    let context = &start["context"];
    let lines = native_int(&context["spins"]["lines"])?;
    ensure!(
        lines > 0
            && array(&settings["lines"])
                .iter()
                .any(|v| native_int(v).ok() == Some(lines)),
        "3 Oaks selected lines unavailable"
    );
    let bet = array(&settings["bets"])
        .iter()
        .map(native_int)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|v| *v > 0)
        .min()
        .context("3 Oaks positive native bet selector missing")?;
    let mut params = json!({"bet_per_line":bet,"lines":lines});
    if selected == "normal" {
        return Ok(json!({"name":"spin","params":params}));
    }
    for guard in array(&binding["guards"]) {
        let path = guard["path"]
            .as_str()
            .ok_or_else(|| unknown_event("3 Oaks serializer guard path changed"))?;
        let v = path.split('.').fold(settings, |v, p| &v[p]);
        let valid = match guard["test"].as_str() {
            Some("falsy") => {
                v.is_null()
                    || v == false
                    || v == ""
                    || v.as_f64() == Some(0.0)
                    || v.as_array().is_some_and(Vec::is_empty)
                    || v.as_object().is_some_and(serde_json::Map::is_empty)
            }
            Some("positive") => v
                .as_f64()
                .or_else(|| v.as_str()?.parse().ok())
                .is_some_and(|n| n > 0.0),
            _ => return Err(unknown_event("3 Oaks serializer guard type changed")),
        };
        ensure!(valid, "3 Oaks client serializer prerequisite unavailable");
    }
    let (prefix, id) = selected
        .split_once('-')
        .context("3 Oaks mode needs native selector")?;
    let action = match prefix {
        "buy" => {
            let offered = array(&context["available_buy_bonus"]);
            let mode = offered
                .iter()
                .find(|v| key(v) == id)
                .context("3 Oaks purchase selector unavailable")?;
            let schema = binding["buy_schema"]
                .as_str()
                .ok_or_else(|| unknown_event("3 Oaks purchase serializer missing"))?;
            match schema {
                "flow_numeric_zero_based_no_factor" => {
                    params["selected_mode"] = json!(
                        native_int(mode)?
                            .checked_sub(1)
                            .filter(|n| *n >= 0)
                            .context("3 Oaks negative purchase index")?
                    )
                }
                "direct_numeric"
                | "direct_string"
                | "flow_numeric"
                | "flow_string"
                | "flow_numeric_no_factor" => {
                    params["selected_mode"] = if schema.ends_with("string") {
                        json!(id)
                    } else {
                        json!(native_int(mode)?)
                    };
                    if matches!(schema, "flow_numeric" | "flow_string") {
                        params["bet_factor"] = json!(factor(start, lines)?);
                    }
                }
                "flow_scatter_count" => {
                    let f = factor(start, lines)?;
                    ensure!(
                        lines == f,
                        "3 Oaks scatter serializer line factor unsupported"
                    );
                    params["bet_factor"] = json!(f);
                    params["ante_bet"] = json!(0);
                    params["buy_spin_scatters_count"] = json!(
                        native_int(mode)?
                            .checked_add(3)
                            .context("3 Oaks scatter count overflow")?
                    );
                }
                "direct_plain" | "flow_plain" | "flow_plain_no_factor"
                    if offered.len() == 1 && id == "1" =>
                {
                    if schema == "flow_plain" {
                        params["bet_factor"] = json!(factor(start, lines)?);
                    }
                }
                "direct_paid_feature" if binding["paid_feature_map"][id].is_string() => {
                    params["paid_feature"] = binding["paid_feature_map"][id].clone()
                }
                _ => return Err(unknown_event("3 Oaks purchase serializer unrecognized")),
            }
            "buy_spin"
        }
        "booster" => {
            let mode = array(&context["available_booster"])
                .iter()
                .find(|v| key(v) == id)
                .context("3 Oaks booster selector unavailable")?;
            match binding["booster_schema"].as_str() {
                Some("gr_option") => {
                    params["selected_mode"] = mode.clone();
                    if has(&context["actions"], "booster_spin") {
                        "booster_spin"
                    } else {
                        let price = settings["booster_prices"]
                            .get(id)
                            .context("3 Oaks native ante_bet value missing")?;
                        params["ante_bet"] = price.clone();
                        "spin"
                    }
                }
                Some("ante_only") => {
                    params["ante_bet"] = settings["booster_prices"]
                        .get(id)
                        .context("3 Oaks native ante_bet value missing")?
                        .clone();
                    "spin"
                }
                _ => return Err(unknown_event("3 Oaks booster serializer unrecognized")),
            }
        }
        "ante" if binding["booster_schema"] == "ante_only" => {
            params["ante_bet"] = array(&settings["ante_bet"])
                .iter()
                .find(|v| key(v).replace('.', "_") == id)
                .context("3 Oaks ante selector unavailable")?
                .clone();
            "spin"
        }
        _ => bail!("3 Oaks requested mode unavailable"),
    };
    Ok(json!({"name":action,"params":params}))
}
async fn command(
    ctx: &RunContext,
    endpoint: &str,
    name: &str,
    mut fields: Value,
    round: bool,
) -> Result<Value> {
    let request_id = Uuid::new_v4().simple().to_string();
    fields["command"] = json!(name);
    fields["request_id"] = json!(request_id);
    fields["client_command_timestamp"] = json!(now_ms());
    let headers = BTreeMap::from([
        ("Origin".into(), SITE.into()),
        ("Referer".into(), format!("{SITE}/")),
    ]);
    let response = ctx
        .json(
            &format!("threeoaks_{name}"),
            endpoint,
            &fields,
            headers,
            round,
        )
        .await?;
    if response["request_id"] != request_id || response["command"] != name {
        return Err(unknown_event(format!(
            "3 Oaks {name} response correlation changed"
        )));
    }
    ensure!(
        response["status"]["code"] == "OK",
        "3 Oaks rejected {name}: {}",
        response["status"]
    );
    Ok(response)
}
struct Demo {
    endpoint: String,
    session: String,
    start: Value,
    source: String,
}
async fn open(ctx: &RunContext, game: &str) -> Result<Demo> {
    let details = format!("{SITE}/api/v1/games/{game}");
    let response = ctx.get("threeoaks_metadata", &details).await?;
    ensure!(
        response.status == 200,
        "3 Oaks metadata HTTP {}",
        response.status
    );
    let metadata: Value = serde_json::from_str(&response.body)?;
    ensure!(
        metadata["data"]["name"] == game && metadata["data"]["is_playable"] == true,
        "3 Oaks official demo unavailable"
    );
    let response = ctx
        .get("threeoaks_launcher", &format!("{details}/play?lang=en"))
        .await?;
    ensure!(
        response.status == 200,
        "3 Oaks launcher HTTP {}",
        response.status
    );
    let (_, source) = response
        .body
        .rsplit_once("})(window, {")
        .ok_or_else(|| unknown_event("3 Oaks launcher config boundary changed"))?;
    let config_text = format!("{{{source}");
    let config = Value::deserialize(&mut serde_json::Deserializer::from_str(&config_text))
        .map_err(|_| unknown_event("3 Oaks launcher configuration changed"))?;
    let o = &config["options"];
    let expected =
        format!("//betman-demo.head.3oaks.com/betman-demo/gs/{game}/desktop/{{QUEUE}}/demo/");
    let queue = o["queue"].as_str().context("3 Oaks queue missing")?;
    ensure!(
        o["provider"] == "3oaks"
            && o["project_name"] == "betman-demo"
            && o["wl"] == "demo"
            && o["game_name"] == game
            && config["desktop"]["server_url"] == expected
            && queue.len() == 32
            && queue
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "3 Oaks launcher did not confirm official demo profile"
    );
    let token = o["token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("3 Oaks demo token missing")?;
    let endpoint = format!("https:{}", expected.replace("{QUEUE}", queue));
    let login = command(
        ctx,
        &endpoint,
        "login",
        json!({"token":token,"language":"en"}),
        false,
    )
    .await?;
    let session = login["session_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("3 Oaks session id missing")?
        .to_owned();
    ensure!(
        has(&login["modes"], "auto"),
        "3 Oaks auto demo mode unavailable"
    );
    ctx.save_session(&json!({"endpoint":endpoint,"session_id":session}))?;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let start = command(
        ctx,
        &endpoint,
        "start",
        json!({"session_id":session,"mode":"auto","huid":Uuid::new_v4().simple().to_string()}),
        false,
    )
    .await?;
    ensure!(
        start["context"]["round_finished"] == true,
        "3 Oaks fresh demo is not at native initial state"
    );
    let source = config["desktop"]["client_url"]
        .as_str()
        .context("3 Oaks client URL missing")?;
    let pattern = regex::Regex::new(&format!(
        r"^https://static\.3oaks\.com/gs/clients_[a-z0-9_]+/{game}/[a-zA-Z0-9_.-]+/$"
    ))?;
    ensure!(
        pattern.is_match(source),
        "3 Oaks client URL left official game path"
    );
    Ok(Demo {
        endpoint,
        session,
        start,
        source: format!("{source}src/game.js"),
    })
}
fn next_action(response: &Value, previous: &Value, binding: &Value) -> Result<Option<Value>> {
    let state = &response["context"];
    if state["last_action"] != previous["name"] {
        return Err(unknown_event(
            "3 Oaks native action response not correlated",
        ));
    }
    match state["round_finished"].as_bool() {
        Some(true) => {
            if state["current"] != "spins" || !has(&state["actions"], "spin") {
                return Err(unknown_event("3 Oaks terminal context changed"));
            }
            return Ok(None);
        }
        Some(false) => {}
        None => {
            return Err(unknown_event(
                "3 Oaks round_finished missing or changed type",
            ));
        }
    }
    let [only] = array(&state["actions"]) else {
        return Err(unknown_event(
            "3 Oaks continuation requires an unrecognized decision",
        ));
    };
    let name = only
        .as_str()
        .ok_or_else(|| unknown_event("3 Oaks continuation action type changed"))?;
    if !matches!(
        name,
        "respin"
            | "bonus_init"
            | "bonus_spins_stop"
            | "freespin_init"
            | "freespin"
            | "freespin_stop"
    ) && !has(&binding["empty_actions"], name)
    {
        return Err(unknown_event(format!(
            "3 Oaks continuation lacks serializer: {name}"
        )));
    }
    Ok(Some(json!({"name":name,"params":{}})))
}
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    ensure!(supports(game, mode), "unsupported 3 Oaks mode");
    let demo = open(ctx, &game.id).await?;
    let source = ctx.get("threeoaks_client_source", &demo.source).await?;
    ensure!(source.status == 200, "3 Oaks client HTTP {}", source.status);
    if source.body.is_empty() || source.body.trim_start().starts_with('<') {
        return Err(unknown_event("3 Oaks client source missing"));
    }
    let hash = sha256(source.body.as_bytes());
    let serializers = serializers();
    let binding = &serializers[&hash];
    if mode.key != "normal" && (binding.is_null() || binding["game_id"] != game.id) {
        return Err(unknown_event(
            "3 Oaks client version has no verified mode serializer",
        ));
    }
    let continuations = continuations();
    let continuation = &continuations[&hash];
    if !continuation.is_null() && continuation["game_id"] != game.id {
        return Err(unknown_event(
            "3 Oaks continuation binding points to another game",
        ));
    }
    ctx.note("threeoaks_client", &json!({"sha256":hash,"mode":mode.key}))?;
    let mut action = entry(&demo.start, binding, &mode.key)?;
    ensure!(
        action["name"]
            .as_str()
            .is_some_and(|name| has(&demo.start["context"]["actions"], name)),
        "3 Oaks requested initial action not offered"
    );
    for _ in 0..ctx.max_steps() {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let response=command(ctx,&demo.endpoint,"play",json!({"session_id":demo.session,"action":action,"set_denominator":1,"quick_spin":0,"sound":false,"autogame":false,"mobile":"0","portrait":false,"fullscreen":false,"viewportSize":"1280x800"}),true).await?;
        match next_action(&response, &action, continuation)? {
            None => {
                let id = response["context"]
                    .get("round_id")
                    .or_else(|| response.get("round_id"))
                    .filter(|v| v.is_string() || v.is_number())
                    .map(key);
                return Ok(Completion::new(id));
            }
            Some(next) => action = next,
        }
    }
    Err(unknown_event("3 Oaks native continuation limit reached"))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn start() -> Value {
        json!({"settings":{"bets":[1,5],"lines":[20]},"context":{"spins":{"lines":20},"available_buy_bonus":[1],"available_booster":[1],"actions":["spin","buy_spin","booster_spin"]}})
    }
    #[test]
    fn normal_entry_has_no_wallet_or_cost_requirements() {
        assert_eq!(
            entry(&start(), &Value::Null, "normal").unwrap(),
            json!({"name":"spin","params":{"bet_per_line":1,"lines":20}})
        );
    }
    #[test]
    fn direct_purchase_selector_does_not_require_price_or_balance() {
        assert_eq!(
            entry(&start(), &json!({"buy_schema":"direct_numeric"}), "buy-1").unwrap()["params"]["selected_mode"],
            1
        );
    }
    #[test]
    fn request_required_ante_value_is_preserved() {
        let mut s = start();
        s["settings"]["booster_prices"] = json!({"1":2});
        let a = entry(&s, &json!({"booster_schema":"ante_only"}), "booster-1").unwrap();
        assert_eq!(a["params"]["ante_bet"], 2);
    }
    #[test]
    fn native_complete_does_not_require_any_financial_fields() {
        let r = json!({"context":{"last_action":"spin","round_finished":true,"current":"spins","actions":["spin"]}});
        assert!(
            next_action(&r, &json!({"name":"spin"}), &Value::Null)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn unknown_continuation_is_a_typed_event() {
        let r = json!({"context":{"last_action":"spin","round_finished":false,"current":"new","actions":["make_up_action"]}});
        let e = next_action(&r, &json!({"name":"spin"}), &Value::Null).unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn generic_and_hash_bound_continuations_are_exact() {
        let r = json!({"context":{"last_action":"spin","round_finished":false,"actions":["bonus_freespins_stop"]}});
        assert!(next_action(&r, &json!({"name":"spin"}), &Value::Null).is_err());
        assert_eq!(
            next_action(
                &r,
                &json!({"name":"spin"}),
                &json!({"empty_actions":["bonus_freespins_stop"]})
            )
            .unwrap()
            .unwrap()["params"],
            json!({})
        );
    }
}
