//! Native capture connector derived from standalone info/hardfire-kalamba/codigo/kalamba.rs.
//! Published source SHA-256: 94a44342892e545c8d6a2bd15246410b3f335b74f25bc633fe6491bd9c229eec
//! Reference transport: game-runner/src/adapters/kalamba.rs; no runtime dependency.
//! Fresh FUN sessions and 66 normal contracts reviewed against kalamba-scratchpad.
//! Closed allowlist, source hashes and projected native fixtures: bindings/kalamba-normal.json.
use crate::{
    context::{DecodedFrame, NativeSocket, RunContext, SocketPayload},
    model::*,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock};
use url::Url;

const PROVIDER: &str = "hardfire-kalamba";
fn bindings() -> &'static Value {
    static BINDINGS: OnceLock<Value> = OnceLock::new();
    BINDINGS.get_or_init(|| {
        serde_json::from_str(include_str!("../../bindings/kalamba-normal.json"))
            .expect("reviewed Kalamba binding must be valid JSON")
    })
}
fn reviewed_game(id: &str) -> bool {
    bindings()["games"]
        .as_object()
        .is_some_and(|games| games.contains_key(id))
}
const LAUNCH: &str = "https://kalamba.integration.demo.kalambagames.com/games/url/";
const ORIGIN: &str = "https://v.play.test.kl.luckycdn.net";

struct Launch {
    url: Url,
    origin: String,
    token: String,
    username: String,
}

pub fn validate(game: &Game, mode: &Mode, settings: &Value) -> Result<()> {
    ensure!(
        reviewed_game(&game.id) && mode.kind == ModeKind::Normal && mode.key == "normal",
        "Kalamba supports only reviewed normal games"
    );
    ensure!(
        settings.is_null() || settings.as_object().is_some_and(|o| o.is_empty()),
        "Kalamba public binding has no overrides"
    );
    Ok(())
}
fn launch_binding(url: &Url, game: &str) -> Result<Option<Launch>> {
    ensure!(
        reviewed_game(game),
        "Kalamba game has no reviewed normal binding"
    );
    crate::context::validate_target(PROVIDER, url.as_str())?;
    ensure!(url.scheme() == "https", "Kalamba launch must use HTTPS");
    let mut query: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (key, value) in url.query_pairs() {
        query
            .entry(key.into_owned())
            .or_default()
            .push(value.into_owned());
    }
    if !query.contains_key("token") {
        return Ok(None);
    }
    let path = match url.host_str() {
        Some("v.play.test.kl.luckycdn.net") => "/1.26.1/index.html",
        Some("v.games.test.kl.luckycdn.net") => "/games/128.8.0/index.html",
        // Official FUN launcher 302, retained in Actions 43324094682.
        Some("awscdn.test.kalamba.net") => "/index.html",
        _ => {
            return Err(crate::model::unknown_event(
                "Kalamba unreviewed launch origin".to_string(),
            ));
        }
    };
    ensure!(url.path() == path, "Kalamba launch client version changed");
    let one = |key: &str| -> Result<&str> {
        let values = query
            .get(key)
            .context("Kalamba missing demo launch field")?;
        ensure!(
            values.len() == 1 && !values[0].is_empty(),
            "Kalamba ambiguous demo launch field"
        );
        Ok(&values[0])
    };
    for (key, expected) in [
        ("gameCode", game),
        ("cageCode", "FUN"),
        ("playMode", "FUN"),
        ("operatorCode", "kalamba-demo"),
        ("currency", "EUR"),
    ] {
        ensure!(
            one(key)? == expected,
            "Kalamba demo launch identity mismatch"
        );
    }
    let token = one("token")?.to_owned();
    let username = one("username")?.to_owned();
    let mut api = crate::context::validate_target(PROVIDER, one("apiUrl")?)?;
    if !(api.scheme() == "wss" && api.path() == "/oryxreplica/" && api.query().is_none()) {
        return Err(crate::model::unknown_event(
            "Kalamba unreviewed WSS launch binding".to_string(),
        ));
    };
    api.query_pairs_mut().extend_pairs([
        ("cageCode", "FUN"),
        ("gameCode", game),
        ("operatorCode", "kalamba-demo"),
        ("playMode", "FUN"),
        ("token", &token),
        ("username", &username),
    ]);
    Ok(Some(Launch {
        url: api,
        origin: if url.host_str() == Some("awscdn.test.kalamba.net") {
            // A browser at this reviewed document sends this origin. Server
            // acceptance must still be established by the actual WSS upgrade.
            url.origin().ascii_serialization()
        } else {
            ORIGIN.into()
        },
        token,
        username,
    }))
}
async fn bootstrap(ctx: &RunContext, game: &str) -> Result<Launch> {
    let mut url = Url::parse(LAUNCH)?;
    url.query_pairs_mut().extend_pairs([
        ("gameCode", game),
        ("cageCode", "FUN"),
        ("currency", "EUR"),
    ]);
    for _ in 0..6 {
        if let Some(binding) = launch_binding(&url, game)? {
            return Ok(binding);
        }
        let response = ctx
            .http(
                "demo_launch",
                "GET",
                url.as_str(),
                BTreeMap::from([("user-agent".into(), "Mozilla/5.0".into())]),
                String::new(),
                false,
            )
            .await?;
        ensure!(
            matches!(response.status, 301 | 302 | 303 | 307 | 308),
            "Kalamba demo launch did not return a redirect"
        );
        let location = response
            .headers
            .get("location")
            .filter(|s| !s.is_empty())
            .context("Kalamba demo launch redirect missing")?;
        url = url.join(location)?;
    }
    bail!("Kalamba demo launch exceeded six steps")
}
fn profile(game: &str, _account: &Value, opened: &Value, initial: &Value) -> Result<()> {
    let state = &initial["data"]["gameState"];
    let config = &initial["data"]["additionalConfigData"];
    ensure!(
        initial["event"] == "OPEN_GAME"
            && opened["gameCode"] == game
            && state["gameId"] == game
            && state["stateTypeThisRound"] == "BaseGame"
            && state["roundNumber"] == 0
            && state["freeSpinCount"] == 0
            && state["stateTypeNextRound"] == "BaseGame"
            && config["availableMultipliersPerBaseBet"]["50"]
                .as_array()
                .is_some_and(|v| v.contains(&json!(4))),
        "Kalamba native default offer missing or session not idle"
    );
    Ok(())
}
fn settlement(game: &str, receipt: &Value) -> Result<Completion> {
    let state = &receipt["data"]["gameState"];
    ensure!(
        receipt["event"] == "SPIN_RESULT"
            && receipt["correlationSeqId"] == 1
            && state["gameId"] == game
            && state["roundNumber"] == 1,
        "Kalamba native response identity mismatch"
    );
    if state["stateTypeThisRound"] != "BaseGame"
        || state["stateTypeNextRound"] != "BaseGame"
        || state["freeSpinCount"] != 0
    {
        return Err(crate::model::unknown_event(format!(
            "Kalamba continuation requires native control: {}",
            state
        )));
    }
    Ok(Completion {
        native_round_id: state["roundNumber"].as_u64().map(|v| v.to_string()),
        details: json!({"game":game,"base_bet":50,"multiplier":4}),
    })
}
fn normal_action() -> Value {
    json!({"data":{"bet":{"baseBet":50,"betMultiplier":4}},"seqId":1,"action":"SPIN"})
}
#[derive(Clone, Copy)]
enum Expected {
    Authenticate,
    Open,
    Spin,
}
#[derive(Default)]
struct Session {
    counter: u64,
}
impl Session {
    fn packet(
        &mut self,
        name: &str,
        client: u64,
        body: Value,
        action: bool,
        data_type: u64,
    ) -> Result<SocketPayload> {
        self.counter = self
            .counter
            .checked_add(1)
            .context("Kalamba message counter overflow")?;
        let mut header = json!({"cId":client,"dType":if action {2}else{data_type},"name":name,"mId":self.counter});
        if action {
            header["dId"] = json!(1);
        }
        Ok(SocketPayload::Text(serde_json::to_string(
            &json!({"header":header,"body":body}),
        )?))
    }
    async fn transact(
        &mut self,
        ctx: &RunContext,
        socket: &mut NativeSocket,
        label: &str,
        payload: SocketPayload,
        expected: Expected,
        round_action: bool,
    ) -> Result<Value> {
        let mut opened = None;
        let mut initial = None;
        ctx.ws_codec_controlled(socket, label, payload, round_action, None, |frame| {
            let text = match frame {
                SocketPayload::Text(s) => s,
                SocketPayload::Binary(_) => bail!("Kalamba expected native JSON text"),
            };
            let packet: Value =
                serde_json::from_str(text).context("Kalamba malformed native JSON")?;
            let header = &packet["header"];
            let body = &packet["body"];
            let name = header["name"]
                .as_str()
                .context("Kalamba native message name missing")?;
            if name == "Ping" {
                let reply = self.packet(
                    "Ping",
                    header["cId"].as_u64().unwrap_or(0),
                    json!({}),
                    false,
                    header["dType"].as_u64().unwrap_or(1),
                )?;
                return Ok(DecodedFrame {
                    values: Vec::new(),
                    replies: vec![reply],
                });
            }
            ensure!(
                header["code"] == 1 && body["event"] != "SPIN_ERROR",
                "Kalamba native request rejected"
            );
            if name == "GameEvent"
                && !matches!(body["event"].as_str(), Some("OPEN_GAME" | "SPIN_RESULT"))
            {
                return Err(crate::model::unknown_event(format!(
                    "Kalamba unknown game event {}",
                    body["event"]
                )));
            }
            let value = match expected {
                Expected::Authenticate if name == "Authenticate" => Some(body.clone()),
                Expected::Spin if name == "GameEvent" && body["event"] == "SPIN_RESULT" => {
                    Some(body.clone())
                }
                Expected::Open => {
                    if name == "OpenGame" {
                        ensure!(
                            opened.is_none(),
                            "Kalamba duplicate OpenGame acknowledgement"
                        );
                        opened = Some(body.clone());
                    }
                    if name == "GameEvent" && body["event"] == "OPEN_GAME" {
                        ensure!(initial.is_none(), "Kalamba duplicate OPEN_GAME state");
                        initial = Some(body.clone());
                    }
                    if let (Some(opened), Some(initial)) = (&opened, &initial) {
                        Some(json!({"opened":opened,"initial":initial}))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            Ok(DecodedFrame {
                values: value.into_iter().collect(),
                replies: Vec::new(),
            })
        })
        .await
    }
}
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    validate(game, mode, settings)?;
    let launch = bootstrap(ctx, &game.id).await?;
    let mut socket = ctx
        .websocket(
            launch.url.as_str(),
            &launch.origin,
            None,
            BTreeMap::from([("User-Agent".into(), "Mozilla/5.0".into())]),
        )
        .await?;
    let mut session = Session::default();
    let auth=session.packet("Authenticate",1,json!({"cageCode":"FUN","clientType":3,"languageCode":"ENG","playMode":2,"reconnect":false,"token":launch.token,"version":"128.8.0"}),false,1)?;
    let account = session
        .transact(
            ctx,
            &mut socket,
            "authenticate",
            auth,
            Expected::Authenticate,
            false,
        )
        .await?;
    let open = session.packet("OpenGame", 2, json!({"gameCode":game.id}), false, 1)?;
    let initialized = session
        .transact(ctx, &mut socket, "open_game", open, Expected::Open, false)
        .await?;
    profile(
        &game.id,
        &account,
        &initialized["opened"],
        &initialized["initial"],
    )?;
    ctx.save_session(&json!({"provider":PROVIDER,"game":game.id,"demo":"FUN","currency":"EUR","token":launch.token,"username":launch.username,"websocket_url":launch.url.as_str(),"origin":launch.origin,
        "account":account,"initialization":initialized,"bet_selection":"reviewed_fixed_bet","base_bet":50,"multiplier":4}))?;
    let action = session.packet("GameAction", 3, normal_action(), true, 2)?;
    let receipt = session
        .transact(ctx, &mut socket, "normal", action, Expected::Spin, true)
        .await?;
    // Continuations are not specified in this normal-only contract. If the
    // provider opens a feature, keep this exact session and attempt for review.
    settlement(&game.id, &receipt)
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    reviewed_game(&game.id) && mode.kind == ModeKind::Normal && mode.key == "normal"
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    const GAME: &str = "9blazingcashpots50k";

    #[test]
    fn all_reviewed_native_normal_games_are_supported() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../bindings/kalamba-normal.json")).unwrap();
        let games = fixtures["games"].as_object().unwrap();
        assert_eq!(games.len(), 66);
        let mode: Mode = serde_json::from_value(json!({"key":"normal","type":"normal"})).unwrap();
        for id in games.keys() {
            let game: Game = serde_json::from_value(json!({"id":id,"modes":[]})).unwrap();
            assert!(supports(&game, &mode), "reviewed normal {id}");
            validate(&game, &mode, &Value::Null).unwrap();
        }
    }
    #[test]
    fn every_reviewed_init_and_close_matches_its_game_without_financial_fields() {
        assert_eq!(normal_action(), bindings()["request"]);
        for (id, fixture) in bindings()["games"].as_object().unwrap() {
            profile(id, &Value::Null, &fixture["opened"], &fixture["initial"]).unwrap();
            let complete = settlement(id, &fixture["spin"]).unwrap();
            assert_eq!(complete.native_round_id.as_deref(), Some("1"), "{id}");
            assert_eq!(complete.details["game"], id.as_str());
            assert_eq!(
                fixture["spin"]["data"]["gameState"]["lastPlacedMainGameBet"],
                bindings()["request"]["data"]["bet"]
            );
            let mut stale = fixture["initial"].clone();
            stale["data"]["gameState"]["roundNumber"] = json!(1);
            assert!(profile(id, &Value::Null, &fixture["opened"], &stale).is_err());
            for (key, value) in [
                ("stateTypeNextRound", json!("FreeSpins")),
                ("freeSpinCount", json!(1)),
                ("stateTypeThisRound", json!("UnknownFeature")),
            ] {
                let mut continuation = fixture["spin"].clone();
                continuation["data"]["gameState"][key] = value;
                assert!(
                    settlement(id, &continuation)
                        .unwrap_err()
                        .downcast_ref::<crate::model::UnknownEvent>()
                        .is_some(),
                    "{id}/{key}"
                );
            }
        }
    }
    #[test]
    fn native_request_and_launch_identity_follow_selected_game() {
        for id in bindings()["games"].as_object().unwrap().keys() {
            let mut url =
                Url::parse("https://v.games.test.kl.luckycdn.net/games/128.8.0/index.html")
                    .unwrap();
            url.query_pairs_mut().extend_pairs([
                ("gameCode", id.as_str()),
                ("cageCode", "FUN"),
                ("playMode", "FUN"),
                ("operatorCode", "kalamba-demo"),
                ("currency", "EUR"),
                ("token", "offline-fixture-token"),
                ("username", "offline-fixture-user"),
                (
                    "apiUrl",
                    "wss://gameserver-api-1.test.twmc8fvg.click/oryxreplica/",
                ),
            ]);
            let launch = launch_binding(&url, id).unwrap().unwrap();
            assert_eq!(
                launch
                    .url
                    .query_pairs()
                    .find(|(key, _)| key == "gameCode")
                    .unwrap()
                    .1,
                id.as_str()
            );
            let mut session = Session::default();
            let SocketPayload::Text(packet) = session
                .packet("GameAction", 3, normal_action(), true, 2)
                .unwrap()
            else {
                panic!()
            };
            let packet: Value = serde_json::from_str(&packet).unwrap();
            assert_eq!(
                packet["header"],
                json!({"cId":3,"dType":2,"name":"GameAction","mId":1,"dId":1})
            );
            assert_eq!(packet["body"], bindings()["request"]);
            url.query_pairs_mut().append_pair("gameCode", "mismatch");
            assert!(launch_binding(&url, id).is_err());
        }
    }
    #[test]
    fn allowlist_rejects_other_games_purchases_and_unreviewed_settings() {
        let normal: Mode = serde_json::from_value(json!({"key":"normal","type":"normal"})).unwrap();
        for id in ["fireeaglereborn", "ageofdragons", "unreviewed-game"] {
            let game: Game = serde_json::from_value(json!({"id":id,"modes":[]})).unwrap();
            assert!(!supports(&game, &normal));
            assert!(validate(&game, &normal, &Value::Null).is_err());
        }
        let game: Game = serde_json::from_value(json!({"id":GAME,"modes":[]})).unwrap();
        for kind in [ModeKind::Purchase, ModeKind::Booster, ModeKind::AnteBet] {
            let mode = Mode {
                kind,
                ..normal.clone()
            };
            assert!(!supports(&game, &mode));
            assert!(validate(&game, &mode, &Value::Null).is_err());
        }
        assert!(validate(&game, &normal, &json!({"baseBet":100})).is_err());
        let fixture = &bindings()["games"][GAME];
        assert!(
            profile(
                "wrong-game",
                &Value::Null,
                &fixture["opened"],
                &fixture["initial"]
            )
            .is_err()
        );
        assert!(settlement("wrong-game", &fixture["spin"]).is_err());
        let mut receipt = fixture["spin"].clone();
        receipt["correlationSeqId"] = json!(2);
        assert!(settlement(GAME, &receipt).is_err());
        receipt["correlationSeqId"] = json!(1);
        receipt["data"]["gameState"]["roundNumber"] = json!(2);
        assert!(settlement(GAME, &receipt).is_err());
        let mut initial = fixture["initial"].clone();
        initial["data"]["additionalConfigData"]["availableMultipliersPerBaseBet"]["50"] =
            json!([1, 2]);
        assert!(profile(GAME, &Value::Null, &fixture["opened"], &initial).is_err());
    }
    #[test]
    fn native_close_without_balance_or_win_and_novel_feature() {
        let mut receipt = json!({"event":"SPIN_RESULT","correlationSeqId":1,"data":{"gameState":{"gameId":GAME,"roundNumber":1,"stateTypeThisRound":"BaseGame","stateTypeNextRound":"BaseGame","freeSpinCount":0}}});
        assert_eq!(
            settlement(GAME, &receipt)
                .unwrap()
                .native_round_id
                .as_deref(),
            Some("1")
        );
        receipt["data"]["gameState"]["stateTypeNextRound"] = json!("NovelFeature");
        assert!(
            settlement(GAME, &receipt)
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
