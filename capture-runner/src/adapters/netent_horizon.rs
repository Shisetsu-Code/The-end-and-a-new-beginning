//! Native capture protocol from standalone info/netent/codigo/netent_horizon.rs.
//! Published evidence SHA-256: fd7ba4945f35e07beae3a23ac3a697e71084ad1db0968f8dae6d1f776be5f85c
//! Reference transport game-runner/src/adapters/netent_horizon.rs; no runtime dependency.
use crate::{
    context::{DecodedFrame, NativeSocket, RunContext, SocketPayload},
    model::{Completion, Game, Mode, ModeKind, decimal, now_ms},
};
use anyhow::{Context, Result, ensure};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock};
use url::Url;
use uuid::Uuid;

const ORIGIN: &str = "https://showcase.evo-games.com";

#[derive(Deserialize)]
struct Binding {
    table_id: String,
    game_type: String,
    client_version: String,
    config_version: String,
    modes: Vec<String>,
    #[serde(default)]
    v1_modes: BTreeMap<String, V1Mode>,
}

#[derive(Deserialize)]
struct V1Mode {
    bet_code: String,
    game: Value,
    asset: String,
    integrity: String,
    source_sha256: String,
    frontend_app: String,
    game_version: String,
    jungle_choice: Option<JungleChoice>,
    finn_choice: Option<FinnChoice>,
}

#[derive(Deserialize)]
struct FinnChoice {
    id: String,
    keys: u64,
}

impl FinnChoice {
    fn keys(&self, state: &Value) -> Result<u64> {
        let game = &state["game"];
        let menu = game["freeSpins"]
            .as_array()
            .context("Finn choice menu missing")?;
        for (id, keys) in [(self.id.as_str(), self.keys), ("randomwilds", 1)] {
            let mut matching = menu.iter().filter(|offer| offer["id"] == id);
            ensure!(
                matching
                    .next()
                    .is_some_and(|offer| offer["keysToUnlock"] == keys)
                    && matching.next().is_none(),
                "Finn choice threshold changed"
            );
        }
        game["totalKeys"]
            .as_u64()
            .context("Finn native key balance missing")
    }

    fn select(&self, state: &Value, allow_preparation: bool) -> Result<&str> {
        let keys = self.keys(state)?;
        ensure!(keys >= 1, "Finn did not open a reviewed choice");
        ensure!(
            keys >= self.keys || allow_preparation,
            "Finn requested choice became locked during play"
        );
        Ok(if keys >= self.keys {
            &self.id
        } else {
            "randomwilds"
        })
    }
}

#[derive(Deserialize)]
struct JungleChoice {
    animal: String,
    gamble: bool,
}

impl JungleChoice {
    fn continuation(&self, response: &Value, stage: usize) -> Result<Value> {
        if !(matches!(self.animal.as_str(), "3" | "4" | "5" | "6" | "7")) {
            return Err(crate::model::unknown_event(
                "unreviewed Jungle Spirit animal".to_string(),
            ));
        };
        let animal = response["game"]
            .get("featuredAnimal")
            .context("Jungle Spirit animal state missing")?;
        let payload = match stage {
            0 if animal.is_null() => json!({"featureBuy":false,"animal":self.animal}),
            1 if animal["id"]
                .as_u64()
                .is_some_and(|id| id.to_string() == self.animal)
                || animal["id"].as_str() == Some(self.animal.as_str()) =>
            {
                json!({"featureBuy":false,"gamble":self.gamble})
            }
            _ => {
                return Err(crate::model::unknown_event(
                    "Jungle Spirit did not acknowledge the reviewed decision sequence",
                ));
            }
        };
        Ok(json!({"continue":{"game":payload}}))
    }
}

impl V1Mode {
    fn asset_url(&self, launch: &super::netent_launch::DemoLaunch) -> Result<Url> {
        if !(launch.config["frontend_app"] == self.frontend_app
            && launch.config["game_version"] == self.game_version)
        {
            return Err(crate::model::unknown_event(
                "Horizon game release changed".to_string(),
            ));
        };
        ensure!(
            self.asset.starts_with("ne")
                && self.asset.ends_with(".js")
                && self
                    .asset
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
            "invalid Horizon source binding"
        );
        let path = format!("js/{}", self.asset);
        let sri = regex::Regex::new(&format!(
            r#""{}"\s*:\s*"{}""#,
            regex::escape(&path),
            regex::escape(&self.integrity)
        ))?;
        ensure!(
            sri.is_match(&launch.frontend_body),
            "Horizon client no longer advertises the reviewed source"
        );
        Ok(launch.frontend.join(&path)?)
    }
}

fn binding(game: &str) -> Option<&'static Binding> {
    static BINDINGS: OnceLock<BTreeMap<String, Binding>> = OnceLock::new();
    BINDINGS
        .get_or_init(|| {
            serde_json::from_slice(include_bytes!("../../bindings/netent-horizon.json"))
                .expect("reviewed NetEnt Horizon contracts")
        })
        .get(game)
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    matches!(
        mode.kind,
        ModeKind::Normal
            | ModeKind::Purchase
            | ModeKind::Variant
            | ModeKind::AnteBet
            | ModeKind::Booster
    ) && binding(&game.id).is_some_and(|row| row.modes.contains(&mode.key))
}

fn socket_url(config: &Value, binding: &Binding, wallet: &str) -> Result<Url> {
    ensure!(
        wallet.starts_with("demo-")
            && config["userType"] == 0
            && config["table_id"] == binding.table_id
            && config["game_provider"] == "netent"
            && config["game_engine"] == "slots",
        "invalid Horizon demo config"
    );
    let path = config["wsUrl"]
        .as_str()
        .filter(|url| url.starts_with("/public/horizon?"))
        .context("unexpected Horizon socket template")?;
    let mut url = Url::parse("wss://showcase.evo-games.com")?.join(path)?;
    let parts: Vec<_> = url.query_pairs().collect();
    let query: BTreeMap<_, _> = parts
        .iter()
        .map(|(k, v)| (k.as_ref(), v.as_ref()))
        .collect();
    ensure!(
        parts.len() == 3
            && query.get("tableId").copied() == Some(binding.table_id.as_str())
            && query.get("gameType").copied() == Some(binding.game_type.as_str())
            && query.get("EVOSESSIONID").is_some_and(|s| !s.is_empty()
                && s.len() <= 4096
                && !s.chars().any(char::is_control))
            && url.fragment().is_none(),
        "Horizon socket game or session differs from its launch"
    );
    let first = Uuid::new_v4().simple().to_string();
    let second = Uuid::new_v4().simple().to_string();
    let instance = format!("{}-{}-{}", &first[..6], &first[6..22], &second[..16]);
    url.query_pairs_mut()
        .append_pair("balanceId", wallet)
        .append_pair("instance", &instance)
        .append_pair("client_version", &binding.client_version);
    Ok(url)
}

fn money(value: &Value) -> Result<Decimal> {
    if let Some(text) = value.as_str() {
        decimal(text)
    } else {
        decimal(&value.to_string())
    }
}

fn quote(initial: &Value, mode: &str, reviewed: Option<&V1Mode>) -> Result<(Value, Decimal)> {
    ensure!(
        initial["nextAction"] == "spin",
        "Horizon demo already has an open round"
    );
    let offers = initial["stakeIncrements"]
        .as_array()
        .context("Horizon stake menu missing")?;
    let mut minimum = None;
    for wire in offers {
        ensure!(wire.is_number(), "Horizon stake must be numeric");
        let amount = money(wire)?;
        ensure!(amount > Decimal::ZERO, "invalid Horizon stake");
        if minimum
            .as_ref()
            .is_none_or(|(current, _)| amount < *current)
        {
            minimum = Some((amount, wire));
        }
    }
    let (stake, wire) = minimum.context("empty Horizon stake menu")?;
    let mut play = json!({"stake":wire,"betCode":mode});
    let protocol = &initial["protocolVersion"];
    let v1 = protocol.is_null() || protocol == 1;
    if let Some(reviewed) = reviewed {
        ensure!(
            v1 && initial["game"]["featureBuyEnabled"] == true,
            "Horizon v1 mode disabled"
        );
        play["betCode"] = json!(reviewed.bet_code);
        play["game"] = reviewed.game.clone();
    } else if protocol == 2 {
        ensure!(
            mode == "Spin" || initial["featuresEnabled"] == true,
            "Horizon feature disabled"
        );
    } else if !(v1 && mode == "Spin") {
        return Err(crate::model::unknown_event(
            "Horizon unknown protocol version",
        ));
    }
    let cost = stake;
    Ok((play, cost))
}

struct Ledger {
    ping_at: u128,
}
impl Ledger {
    fn decode(&mut self, frame: &SocketPayload) -> Result<(Value, Vec<SocketPayload>)> {
        let SocketPayload::Text(text) = frame else {
            return Err(crate::model::unknown_event(
                "Horizon unexpected binary frame",
            ));
        };
        let packet: Value = serde_json::from_str(text)?;
        let body = &packet["payload"];
        if matches!(
            body["type"].as_str(),
            Some("connection.kickout" | "SlotsError" | "error" | "Error")
        ) {
            anyhow::bail!("Horizon rejected native action");
        }
        // balanceUpdated is retained raw by RunContext and has no completion gate.
        let now = now_ms() as u128;
        let replies = if now >= self.ping_at {
            self.ping_at = now + 5000;
            vec![SocketPayload::Text(json!({"recipient":"ping-pong","payload":{"id":short_id(),"type":"metrics.ping","args":{"t":now}}}).to_string())]
        } else {
            Vec::new()
        };
        Ok((packet, replies))
    }
}

fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..10].to_owned()
}

async fn call(
    ctx: &RunContext,
    socket: &mut NativeSocket,
    ledger: &mut Ledger,
    label: &str,
    details: Value,
) -> Result<Value> {
    let id = Uuid::new_v4().to_string();
    let packet = json!({"id":id,"recipient":"rng-slots","payload":{"id":short_id(),"type":"SlotsCall","details":details}});
    ctx.ws_codec_controlled(
        socket,
        label,
        SocketPayload::Text(packet.to_string()),
        true,
        None,
        |frame| {
            let (packet, replies) = ledger.decode(frame)?;
            let body = &packet["payload"];
            let mut values = Vec::new();
            if body["type"] == "SlotsResult" && packet["replyTo"] == id {
                let result = &body["details"]["params"];
                ensure!(result.is_object(), "Horizon result payload missing");
                values.push(result.clone());
            }
            Ok(DecodedFrame { values, replies })
        },
    )
    .await
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    let budget = settings["max_prerequisite_rounds"].as_u64().unwrap_or(0);
    let binding = binding(&game.id).context("unreviewed Horizon game")?;
    let launch = super::netent_launch::open_config(
        ctx,
        &game.id,
        &binding.table_id,
        "horizon",
        &binding.config_version,
    )
    .await?;
    let selector = binding.v1_modes.get(&mode.key);
    if let Some(reviewer) = selector {
        let url = reviewer.asset_url(&launch)?;
        let source = ctx
            .get("netent_horizon_reviewed_source", url.as_str())
            .await?;
        if !(source.status == 200
            && crate::model::sha256(source.body.as_bytes()) == reviewer.source_sha256)
        {
            return Err(crate::model::unknown_event(
                "Horizon reviewed bundle changed or is unavailable".to_string(),
            ));
        };
    }
    let wallet = &launch.wallet;
    let url = socket_url(&launch.config, binding, wallet)?;
    let mut ledger = Ledger { ping_at: 0 };
    let mut socket = ctx
        .websocket(url.as_str(), ORIGIN, None, BTreeMap::new())
        .await?;
    let app = json!({"id":Uuid::new_v4().to_string(),"recipient":"horizon","payload":{"type":"AppInfo","recipient":"rng-slots"}});
    let mut initial = None;
    let setup = ctx
        .ws_codec_controlled(
            &mut socket,
            "netent_horizon_app_info",
            SocketPayload::Text(app.to_string()),
            false,
            None,
            |frame| {
                let (packet, replies) = ledger.decode(frame)?;
                let body = &packet["payload"];
                if body["type"] == "SlotsResult"
                    && body["details"]["params"]["stakeIncrements"].is_array()
                {
                    initial = Some(body["details"]["params"].clone());
                }
                let values = initial.take().into_iter().collect();
                Ok(DecodedFrame { values, replies })
            },
        )
        .await?;
    let (play, _cost) = quote(&setup, &mode.key, selector)?;
    let finn = selector.and_then(|reviewed| reviewed.finn_choice.as_ref());
    ensure!(
        finn.is_none() || (1..=100).contains(&budget),
        "Finn prerequisite budget is missing or invalid"
    );
    ctx.save_session(
        &json!({"socket":url.as_str(),"demo_wallet":wallet,"table_id":binding.table_id}),
    )?;
    let mut keys = finn
        .map(|choice| choice.keys(&setup))
        .transpose()?
        .unwrap_or(0);
    let mut previous = setup["gameRoundId"].as_str().map(str::to_owned);
    let mut preparations = Vec::new();
    let mut remaining_steps = ctx.max_steps();
    loop {
        if let Some(choice) = finn {
            ensure!(
                keys >= choice.keys || (preparations.len() as u64) < budget,
                "Finn prerequisite budget exhausted after {} closed purchases",
                preparations.len()
            );
        }
        let request = HorizonPlay {
            details: &play,
            previous: previous.as_deref(),
            selector,
            allow_preparation: finn.is_some_and(|choice| keys < choice.keys),
        };
        let (mut outcome, terminal) =
            settle_round(ctx, &mut socket, &mut ledger, request, &mut remaining_steps).await?;
        let Some(wanted) = finn else {
            return Ok(outcome);
        };
        if outcome.details["selection"] == wanted.id {
            outcome.details["preparation_rounds"] = json!(preparations);
            return Ok(outcome);
        }
        // Persist each closed root before another paid command, including when
        // the campaign stops later on a locked choice or a transport problem.
        let receipt =
            json!({"operation":"buy_bonus","purpose":"finn_key_progression","outcome":outcome});
        ctx.note("netent_finn_preparation_receipt", &receipt)?;
        preparations.push(receipt);
        previous = outcome.native_round_id.take();
        let next = terminal["game"]["totalKeys"]
            .as_u64()
            .context("Finn closing key balance missing")?;
        ensure!(
            next > keys,
            "Finn closed preparation without key progression"
        );
        keys = next;
    }
}

struct HorizonPlay<'a> {
    details: &'a Value,
    previous: Option<&'a str>,
    selector: Option<&'a V1Mode>,
    allow_preparation: bool,
}

async fn settle_round(
    ctx: &RunContext,
    socket: &mut NativeSocket,
    ledger: &mut Ledger,
    request: HorizonPlay<'_>,
    remaining: &mut usize,
) -> Result<(Completion, Value)> {
    let HorizonPlay {
        details: play,
        previous,
        selector,
        allow_preparation,
    } = request;
    ensure!(*remaining > 0, "Horizon action budget exhausted");
    *remaining -= 1;
    let mut terminal = call(
        ctx,
        socket,
        ledger,
        "netent_horizon_play",
        json!({"play":play}),
    )
    .await?;
    let round = terminal["gameRoundId"]
        .as_str()
        .filter(|v| !v.is_empty())
        .context("Horizon round identifier missing")?
        .to_owned();
    ensure!(
        Some(round.as_str()) != previous,
        "Horizon round identifier did not advance"
    );
    let choice = selector.and_then(|reviewed| reviewed.jungle_choice.as_ref());
    let finn = selector.and_then(|reviewed| reviewed.finn_choice.as_ref());
    let mut selection = None;
    let mut decision_stage = 0;
    for _ in 1..ctx.max_steps().min(500) {
        if terminal["nextAction"] == "spin" {
            break;
        }
        recognized_next(&terminal)?;
        let details = match terminal["nextAction"].as_str() {
            Some("respin" | "freespin" | "featureexecution" | "jackpotpayout") => {
                json!({"continue":{}})
            }
            Some("userdecision") if choice.is_some() => {
                let wire = choice.unwrap().continuation(&terminal, decision_stage)?;
                decision_stage += 1;
                wire
            }
            Some("userdecision") if finn.is_some() && selection.is_none() => {
                let selected = finn.unwrap().select(&terminal, allow_preparation)?;
                selection = Some(selected);
                json!({"continue":{"game":{"select":selected}}})
            }
            _ => {
                return Err(crate::model::unknown_event(
                    "Horizon continuation requires an unreviewed choice",
                ));
            }
        };
        ensure!(*remaining > 0, "Horizon action budget exhausted");
        *remaining -= 1;
        terminal = call(ctx, socket, ledger, "netent_horizon_continue", details).await?;
        if !(terminal["gameRoundId"] == round) {
            return Err(crate::model::unknown_event(
                "Horizon changed round during continuation".to_string(),
            ));
        };
    }
    ensure!(
        choice.is_none() || decision_stage == 2,
        "Jungle Spirit did not complete both requested decisions"
    );
    ensure!(
        finn.is_none() || selection.is_some(),
        "Finn did not execute the requested decision sequence"
    );
    if terminal["nextAction"] != "spin" {
        return Err(crate::model::unknown_event(
            "Horizon native round closure not acknowledged",
        ));
    }
    let mut native_data = json!({"play":play});
    if let Some(selected) = selection {
        native_data["selection"] = json!(selected);
        native_data["closing_keys"] = terminal["game"]["totalKeys"].clone();
    }
    let outcome = Completion {
        native_round_id: Some(round),
        details: native_data,
    };
    Ok((outcome, terminal))
}

fn recognized_next(state: &Value) -> Result<()> {
    match state["nextAction"].as_str() {
        Some(
            "spin" | "respin" | "freespin" | "featureexecution" | "jackpotpayout" | "userdecision",
        ) => Ok(()),
        other => Err(crate::model::unknown_event(format!(
            "Horizon unknown next action {other:?}"
        ))),
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn setup_and_native_terminal_never_require_balance_updates() {
        let setup = json!({"nextAction":"spin","stakeIncrements":[0.20,0.10],"protocolVersion":1});
        let (request, _) = quote(&setup, "Spin", None).unwrap();
        assert_eq!(request, json!({"stake":0.10,"betCode":"Spin"}));
        let mut codec = Ledger { ping_at: u128::MAX };
        let packet = json!({"payload":{"type":"SlotsResult","details":{"params":{"gameRoundId":"r1","nextAction":"spin"}}}});
        let (decoded, replies) = codec
            .decode(&SocketPayload::Text(packet.to_string()))
            .unwrap();
        assert!(replies.is_empty());
        assert_eq!(decoded, packet);
        recognized_next(&decoded["payload"]["details"]["params"]).unwrap();
        assert!(
            recognized_next(&json!({"nextAction":"novel_choice"}))
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
    #[test]
    fn known_jungle_choices_follow_native_sequence() {
        let choice = JungleChoice {
            animal: "7".into(),
            gamble: true,
        };
        assert_eq!(
            choice
                .continuation(&json!({"game":{"featuredAnimal":null}}), 0)
                .unwrap(),
            json!({"continue":{"game":{"featureBuy":false,"animal":"7"}}})
        );
        assert!(
            choice
                .continuation(&json!({"game":{"featuredAnimal":{"id":3}}}), 1)
                .is_err()
        );
    }
}
