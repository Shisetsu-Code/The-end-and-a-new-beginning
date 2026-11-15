//! Native capture connector derived from standalone info/hardfire-kalamba/codigo/kalamba.rs.
//! Published source SHA-256: 94a44342892e545c8d6a2bd15246410b3f335b74f25bc633fe6491bd9c229eec
//! Reference transport: game-runner/src/adapters/kalamba.rs; no runtime dependency.
//! Fresh FUN session and one reviewed normal round from collection_claude_extra.py.
use crate::{
    context::{DecodedFrame, NativeSocket, RunContext, SocketPayload},
    model::*,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::Url;

const PROVIDER: &str = "hardfire-kalamba";
const GAME: &str = "9blazingcashpots50k";
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
        game.id == GAME && mode.kind == ModeKind::Normal && mode.key == "normal",
        "Kalamba supports only the reviewed normal game"
    );
    ensure!(
        settings.is_null() || settings.as_object().is_some_and(|o| o.is_empty()),
        "Kalamba public binding has no overrides"
    );
    Ok(())
}
fn launch_binding(url: &Url) -> Result<Option<Launch>> {
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
        ("gameCode", GAME),
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
        ("gameCode", GAME),
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
async fn bootstrap(ctx: &RunContext) -> Result<Launch> {
    let mut url = Url::parse(LAUNCH)?;
    url.query_pairs_mut().extend_pairs([
        ("gameCode", GAME),
        ("cageCode", "FUN"),
        ("currency", "EUR"),
    ]);
    for _ in 0..6 {
        if let Some(binding) = launch_binding(&url)? {
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
fn profile(_account: &Value, opened: &Value, initial: &Value) -> Result<()> {
    let state = &initial["data"]["gameState"];
    let config = &initial["data"]["additionalConfigData"];
    ensure!(
        opened["gameCode"] == GAME
            && state["gameId"] == GAME
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
fn settlement(_profile: &(), receipt: &Value) -> Result<Completion> {
    let state = &receipt["data"]["gameState"];
    ensure!(
        receipt["event"] == "SPIN_RESULT"
            && receipt["correlationSeqId"] == 1
            && state["gameId"] == GAME,
        "Kalamba native response identity mismatch"
    );
    if state["stateTypeNextRound"] != "BaseGame" || state["freeSpinCount"] != 0 {
        return Err(crate::model::unknown_event(format!(
            "Kalamba continuation requires native control: {}",
            state
        )));
    }
    Ok(Completion {
        native_round_id: state["roundNumber"].as_u64().map(|v| v.to_string()),
        details: json!({"game":GAME,"base_bet":50,"multiplier":4}),
    })
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
    let launch = bootstrap(ctx).await?;
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
    let open = session.packet("OpenGame", 2, json!({"gameCode":GAME}), false, 1)?;
    let initialized = session
        .transact(ctx, &mut socket, "open_game", open, Expected::Open, false)
        .await?;
    profile(&account, &initialized["opened"], &initialized["initial"])?;
    ctx.save_session(&json!({"provider":PROVIDER,"game":GAME,"demo":"FUN","currency":"EUR","token":launch.token,"username":launch.username,"websocket_url":launch.url.as_str(),"origin":launch.origin,
        "account":account,"initialization":initialized,"bet_selection":"reviewed_fixed_bet","base_bet":50,"multiplier":4}))?;
    let action = session.packet(
        "GameAction",
        3,
        json!({"data":{"bet":{"baseBet":50,"betMultiplier":4}},"seqId":1,"action":"SPIN"}),
        true,
        2,
    )?;
    let receipt = session
        .transact(ctx, &mut socket, "normal", action, Expected::Spin, true)
        .await?;
    // Continuations are not specified in this normal-only contract. If the
    // provider opens a feature, keep this exact session and attempt for review.
    settlement(&(), &receipt)
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    game.id == GAME && mode.kind == ModeKind::Normal && mode.key == "normal"
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn native_close_without_balance_or_win_and_novel_feature() {
        let mut receipt = json!({"event":"SPIN_RESULT","correlationSeqId":1,"data":{"gameState":{"gameId":GAME,"roundNumber":1,"stateTypeNextRound":"BaseGame","freeSpinCount":0}}});
        assert_eq!(
            settlement(&(), &receipt)
                .unwrap()
                .native_round_id
                .as_deref(),
            Some("1")
        );
        receipt["data"]["gameState"]["stateTypeNextRound"] = json!("NovelFeature");
        assert!(
            settlement(&(), &receipt)
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
