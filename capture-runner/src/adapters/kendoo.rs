//! Native capture protocol derived from standalone info/kendoo/codigo and the evidence reference adapter.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, now_ms, unknown_event},
};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::LazyLock};
use url::Url;

const ORIGIN: &str = "https://betman-demo.head.kendoo.pro";
#[derive(Deserialize)]
struct GameBinding {
    native_code: String,
    modes: Vec<String>,
}

#[derive(Deserialize)]
struct Bindings {
    games: BTreeMap<String, GameBinding>,
}

static BINDINGS: LazyLock<Bindings> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../bindings/kendoo-extra.json"))
        .expect("valid Kendoo native code bindings")
});

pub(crate) fn known_slug(slug: &str) -> bool {
    BINDINGS.games.contains_key(slug)
}

pub(crate) fn native_codes() -> impl Iterator<Item = &'static str> {
    BINDINGS.games.values().map(|g| g.native_code.as_str())
}

fn game_code(slug: &str) -> Option<&'static str> {
    BINDINGS.games.get(slug).map(|g| g.native_code.as_str())
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    let expected = if mode.key == "normal" {
        ModeKind::Normal
    } else if mode.key.starts_with("normal:lines=") {
        ModeKind::Variant
    } else if mode.key.starts_with("ante:") {
        ModeKind::AnteBet
    } else if mode.key.starts_with("buy:") {
        ModeKind::Purchase
    } else {
        return false;
    };
    mode.kind == expected
        && BINDINGS
            .games
            .get(&game.id)
            .is_some_and(|g| g.modes.contains(&mode.key))
}

fn launch(html: &str, code: &str) -> Result<Url> {
    let fragments = Regex::new(r"self\.__next_f\.push\((\[.*?\])\)</script>")?;
    let mut state = String::new();
    for row in fragments.captures_iter(html) {
        let item: Value = serde_json::from_str(&row[1])?;
        if let Some(fragment) = item[1].as_str() {
            state.push_str(fragment);
        }
    }
    let links = Regex::new(r#""linkToDemoWebsite"\s*:\s*("(?:[^"\\]|\\.)*")"#)?;
    let mut target = None;
    for link in links.captures_iter(&state) {
        let url = Url::parse(&serde_json::from_str::<String>(&link[1])?)?;
        if url.origin().ascii_serialization() != ORIGIN
            || url.path() != "/betman-demo/static/game.html"
        {
            continue;
        }
        let matches = |name: &str, value: &str| {
            let mut values = url.query_pairs().filter(|(key, _)| key == name);
            values.next().is_some_and(|(_, v)| v == value) && values.next().is_none()
        };
        if !matches("game", code) || !matches("wl", "demo") {
            continue;
        }
        ensure!(
            target.as_ref().is_none_or(|previous| previous == &url),
            "ambiguous Kendoo demo link"
        );
        target = Some(url);
    }
    target.context("Kendoo demo link missing")
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|v| !v.is_empty())
        .with_context(|| format!("Kendoo {key} missing"))
}

fn amount(value: &Value) -> Result<Decimal> {
    if let Some(v) = value.as_str() {
        return decimal(v);
    }
    ensure!(value.is_number(), "Kendoo amount missing");
    decimal(&value.to_string())
}

struct Session<'a> {
    ctx: &'a RunContext,
    endpoint: Url,
    launch: Url,
    sequence: usize,
}

impl Session<'_> {
    async fn command(&mut self, name: &str, mut body: Value, financial: bool) -> Result<Value> {
        self.sequence += 1;
        let id = self.sequence.to_string();
        body["request_id"] = json!(id);
        body["command"] = json!(name);
        body["client_command_timestamp"] = json!(now_ms());
        let response = self
            .ctx
            .http(
                name,
                "POST",
                self.endpoint.as_str(),
                BTreeMap::from([
                    ("origin".into(), ORIGIN.into()),
                    ("referer".into(), self.launch.to_string()),
                    ("content-type".into(), "text/plain;charset=UTF-8".into()),
                    ("accept".into(), "*/*".into()),
                ]),
                serde_json::to_string(&body)?,
                financial,
            )
            .await?;
        ensure!(
            response.status == 200,
            "Kendoo {name} HTTP {}",
            response.status
        );
        let response: Value = serde_json::from_str(&response.body)
            .map_err(|e| unknown_event(format!("Kendoo response format changed: {e}")))?;
        match response["status"]["code"].as_str() {
            Some("OK") => {}
            Some("SERVER_ERROR") => anyhow::bail!("Kendoo {name} rejected the command"),
            _ => {
                return Err(unknown_event(
                    "Kendoo response status format or code changed",
                ));
            }
        }
        if response["request_id"] != id || response["command"] != name {
            return Err(unknown_event(
                "Kendoo response command or request identity changed",
            ));
        }
        Ok(response)
    }
}

fn entry(start: &Value, mode: &Mode) -> Result<(&'static str, Value)> {
    let settings = &start["settings"];
    let bets = settings["bets"]
        .as_array()
        .ok_or_else(|| unknown_event("Kendoo native bet menu missing"))?;
    let mut minimum = None;
    for bet in bets {
        let n = amount(bet)?;
        ensure!(n > Decimal::ZERO, "invalid Kendoo native stake");
        if minimum.as_ref().is_none_or(|(old, _)| n < *old) {
            minimum = Some((n, bet));
        }
    }
    let (_, bet) = minimum.context("empty Kendoo native bet menu")?;
    let lines_menu = settings["lines"]
        .as_array()
        .ok_or_else(|| unknown_event("Kendoo native lines menu missing"))?;
    let lines = if let Some(requested) = mode.key.strip_prefix("normal:lines=") {
        ensure!(
            mode.kind == ModeKind::Variant,
            "invalid Kendoo line mode kind"
        );
        let selected: u64 = requested.parse()?;
        ensure!(
            selected > 0 && lines_menu.contains(&json!(selected)),
            "Kendoo requested lines are unavailable"
        );
        selected
    } else {
        lines_menu
            .first()
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
            .ok_or_else(|| unknown_event("Kendoo native line count missing"))?
    };
    let mut params = json!({"bet_per_line":bet,"lines":lines});
    let action = if let Some(id) = mode.key.strip_prefix("buy:") {
        ensure!(
            mode.kind == ModeKind::Purchase,
            "invalid Kendoo purchase mode kind"
        );
        let id: u64 = id.parse()?;
        ensure!(
            start["context"]["available_buy_bonus"]
                .as_array()
                .is_some_and(|v| v.contains(&json!(id))),
            "Kendoo requested purchase is unavailable"
        );
        params["selected_mode"] = json!(id);
        "buy_spin"
    } else if let Some(ante) = mode.key.strip_prefix("ante:") {
        ensure!(
            mode.kind == ModeKind::AnteBet,
            "invalid Kendoo ante mode kind"
        );
        let wanted = decimal(ante)?;
        let offered = settings["ante_bet"]
            .as_array()
            .context("Kendoo ante menu unavailable")?;
        let native = offered
            .iter()
            .find(|v| amount(v).is_ok_and(|v| v == wanted))
            .context("Kendoo requested ante is unavailable")?;
        params["ante_bet"] = native.clone();
        "spin"
    } else {
        ensure!(
            (mode.key == "normal" && mode.kind == ModeKind::Normal)
                || (mode.key.starts_with("normal:lines=") && mode.kind == ModeKind::Variant),
            "unrecognized Kendoo entry mode"
        );
        "spin"
    };
    ensure!(
        start["context"]["actions"]
            .as_array()
            .is_some_and(|v| v.contains(&json!(action))),
        "Kendoo entry action is unavailable"
    );
    Ok((action, params))
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &serde_json::Value,
) -> Result<Completion> {
    ensure!(supports(game, mode), "Kendoo mode has no reviewed binding");
    let code = game_code(&game.id).context("Kendoo native game binding missing")?;
    let page = ctx
        .get(
            "official_game",
            &format!("https://kendoo.com/games/{}", game.id),
        )
        .await?;
    ensure!(
        page.status == 200,
        "Kendoo official game HTTP {}",
        page.status
    );
    let launch = launch(&page.body, code)?;
    let html = ctx.get("demo_launch", launch.as_str()).await?;
    ensure!(html.status == 200, "Kendoo launcher HTTP {}", html.status);
    let literal = Regex::new(r"\}\)\(window,\s*(\{)")?;
    let start = literal
        .captures(&html.body)
        .and_then(|v| v.get(1))
        .context("Kendoo bootstrap missing")?
        .start();
    let binding = Value::deserialize(&mut serde_json::Deserializer::from_str(&html.body[start..]))?;
    let options = &binding["options"];
    ensure!(
        options["wl"] == "demo" && options["protocol"] == "goreel",
        "unexpected Kendoo demo protocol"
    );
    let queue = string(options, "queue")?;
    ensure!(
        queue
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "invalid Kendoo queue"
    );
    let endpoint =
        launch.join(&string(&binding["desktop"], "server_url")?.replace("{QUEUE}", queue))?;
    ensure!(
        endpoint.origin().ascii_serialization() == ORIGIN
            && endpoint.path() == format!("/betman-demo/gs/{code}/desktop/{queue}/demo/")
            && endpoint.query().is_none(),
        "unexpected Kendoo endpoint"
    );
    let mut wire = Session {
        ctx,
        endpoint,
        launch,
        sequence: 0,
    };
    let login = wire
        .command(
            "login",
            json!({"token":string(options,"token")?,"language":"en"}),
            false,
        )
        .await?;
    let session = string(&login, "session_id")?;
    let start = wire
        .command(
            "start",
            json!({"session_id":session,"huid":string(&login["user"],"huid")?,"mode":"auto"}),
            false,
        )
        .await?;
    ensure!(
        start["session_id"] == session && start["context"]["round_finished"] == true,
        "Kendoo session is not idle"
    );
    let (first_action, mut params) = entry(&start, mode)?;
    ctx.save_session(&json!({"game":code,"session":session}))?;
    let mut current = start;
    let mut action = first_action.to_owned();

    for index in 0..ctx.max_steps() {
        ensure!(
            current["context"]["actions"]
                .as_array()
                .is_some_and(|v| v.iter().any(|p| p == &action)),
            "Kendoo action not offered"
        );
        current = wire.command("play",json!({"session_id":session,"action":{"name":action,"params":params},
            "set_denominator":1,"quick_spin":2,"sound":false,"autogame":false,"mobile":"0","portrait":false,"fullscreen":false}),true).await?;
        if current["session_id"] != session {
            return Err(unknown_event("Kendoo session identity changed"));
        }
        let context = &current["context"];
        let closed = context["round_finished"]
            .as_bool()
            .ok_or_else(|| unknown_event("Kendoo native round_finished missing"))?;
        if context
            .get("last_action")
            .is_some_and(|last| last != &action)
        {
            return Err(unknown_event(
                "Kendoo native last_action differs from request",
            ));
        }
        if closed {
            return Ok(Completion {
                native_round_id: None,
                details: json!({"game":code,"steps":index+1}),
            });
        }
        action = next_action(context)?.to_owned();
        params = json!({});
    }
    anyhow::bail!("Kendoo continuation limit reached")
}

fn next_action(context: &Value) -> Result<&str> {
    let actions = context["actions"]
        .as_array()
        .ok_or_else(|| unknown_event("Kendoo continuation missing"))?;
    if actions.len() != 1 {
        return Err(unknown_event(format!(
            "Kendoo ambiguous or unknown action menu: {actions:?}"
        )));
    }
    match actions[0].as_str() {
        Some(
            action @ ("bonus_init" | "respin" | "bonus_spins_stop" | "freespin_init" | "freespin"
            | "freespin_stop" | "mini_bonus_init" | "mini_bonus_stop"),
        ) => Ok(action),
        _ => Err(unknown_event(format!(
            "Kendoo unsupported next action: {}",
            actions[0]
        ))),
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    fn mode(key: &str, kind: ModeKind) -> Mode {
        Mode {
            key: key.into(),
            kind,
            label: key.into(),
            requires_prerequisites: Value::Null,
        }
    }

    #[test]
    fn native_menu_selects_buy_ante_and_lines_without_balance() {
        let init = json!({"settings":{"bets":[10,1],"lines":[5,3],"ante_bet":[1.3,1.5]},
            "context":{"actions":["spin","buy_spin"],"available_buy_bonus":[1,2,3,4],"round_finished":true}});
        let (action, params) = entry(&init, &mode("buy:4", ModeKind::Purchase)).unwrap();
        assert_eq!(action, "buy_spin");
        assert_eq!(
            params,
            json!({"bet_per_line":1,"lines":5,"selected_mode":4})
        );
        assert_eq!(
            entry(&init, &mode("ante:1.3", ModeKind::AnteBet))
                .unwrap()
                .1["ante_bet"],
            1.3
        );
        assert_eq!(
            entry(&init, &mode("normal:lines=3", ModeKind::Variant))
                .unwrap()
                .1["lines"],
            3
        );
        assert!(entry(&init, &mode("buy:5", ModeKind::Purchase)).is_err());
        assert!(entry(&init, &mode("ante:2", ModeKind::AnteBet)).is_err());
        assert!(entry(&init, &mode("normal:lines=7", ModeKind::Variant)).is_err());
    }

    #[test]
    fn known_new_free_sequences_and_unobserved_choices() {
        for action in [
            "freespin_init",
            "freespin",
            "freespin_stop",
            "mini_bonus_init",
            "mini_bonus_stop",
        ] {
            assert_eq!(next_action(&json!({"actions":[action]})).unwrap(), action);
        }
        assert!(next_action(&json!({"actions":["mini_bonus_pick"]})).is_err());
    }

    #[test]
    fn free_native_action_needs_no_wallet() {
        assert_eq!(
            next_action(&json!({"round_finished":false,"actions":["respin"]})).unwrap(),
            "respin"
        );
    }
    #[test]
    fn novel_or_mixed_menu_alerts() {
        for actions in [json!(["new"]), json!(["respin", "new"]), json!([])] {
            assert!(
                next_action(&json!({"actions":actions}))
                    .unwrap_err()
                    .downcast_ref::<crate::model::UnknownEvent>()
                    .is_some()
            );
        }
    }
}
