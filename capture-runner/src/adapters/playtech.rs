//! Native GPAS capture. Supplied playtech_engine source is read-only protocol evidence.
//! Fresh HTTP sources/configuration attest every constructor before paid entry.
use crate::{
    context::{DecodedFrame, NativeSocket, RunContext, SocketPayload},
    model::{Completion, Game, Mode, ModeKind, now_ms},
};
use anyhow::{Context, Result, ensure};
use native::{Picks, Step, entry, next_step, source_contract};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::LazyLock};
use uuid::Uuid;

pub(super) const ROOT: &str = "https://gpas-games.playtechone.com";
const CODE: &str = "gpas_blitz10_pop";
const OFFER: &str = "d743f173-2233-463f-909d-a4e95f68a07a";
const COMMAND: &str = "com.pt.casino.platform.game.GameCommand";

static BINDINGS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../bindings/playtech-native.json"))
        .expect("reviewed GPAS bindings")
});
static SUPPORTED: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../bindings/playtech-supported.json"))
        .expect("reviewed GPAS inventory intersection")
});

pub fn supports(game: &Game, mode: &Mode) -> bool {
    let Some(code) = game.id.strip_prefix("gpas:") else {
        return false;
    };
    SUPPORTED["games"][code][&mode.key]["kind"]
        .as_str()
        .and_then(|kind| serde_json::from_value::<ModeKind>(json!(kind)).ok())
        == Some(mode.kind)
}

fn binding(code: &str) -> Value {
    if code == CODE {
        json!({"modes":{OFFER:{"selector":"default","parameters":[{"key":"extraBetSupported","value":"true"},{"key":"extraBetActive","value":"false"}]}}})
    } else {
        BINDINGS.get(code).cloned().unwrap_or(json!({}))
    }
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .filter(|v| !v.is_empty())
        .with_context(|| format!("GPAS {name} missing"))
}

async fn get(ctx: &RunContext, label: &str, url: &str) -> Result<String> {
    let reply = ctx.get(label, url).await?;
    ensure!(reply.status == 200, "GPAS {label} HTTP {}", reply.status);
    Ok(reply.body)
}

async fn exchange(
    ctx: &RunContext,
    socket: &mut NativeSocket,
    label: &str,
    data: Value,
    round_action: bool,
    expected: &str,
) -> Result<Value> {
    let id = format!("tester-{}", Uuid::new_v4().simple());
    let body = format!("3:::{}", json!({"data":data,"correlationId":id}));
    let mut command = None;
    ctx.ws_codec_controlled(
        socket,
        label,
        SocketPayload::Text(body),
        round_action,
        Some(&id),
        |frame| {
            let SocketPayload::Text(frame) = frame else {
                return Err(crate::model::unknown_event(
                    "unexpected GPAS frame".to_string(),
                ));
            };
            let mut result = DecodedFrame {
                values: vec![],
                replies: vec![],
            };
            if frame == "2::" {
                result.replies.push(SocketPayload::Text("2::".into()));
                return Ok(result);
            }
            let packet: Value = serde_json::from_str(
                frame
                    .strip_prefix("3:::")
                    .ok_or_else(|| crate::model::unknown_event("unexpected GPAS frame prefix"))?,
            )?;
            let payload = &packet["data"];
            let ty = field(payload, "_type")?;
            let current = packet["correlationId"] == id;
            if current
                && ty != expected
                && ty != "com.pt.casino.platform.balance.BalanceNotification"
            {
                return Err(crate::model::unknown_event(format!(
                    "GPAS unknown correlated response type {ty}"
                )));
            }
            if current && ty == expected {
                command = Some(payload.clone());
            }
            if let Some(payload) = command.take() {
                result
                    .values
                    .push(json!({"requestId":id,"payload":payload}));
            }
            Ok(result)
        },
    )
    .await
    .map(|v| v["payload"].clone())
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    ensure!(supports(game, mode), "GPAS mode has no reviewed binding");
    let code = game
        .id
        .strip_prefix("gpas:")
        .context("GPAS game ID missing")?;
    let release: Value = serde_json::from_str(
        &get(ctx, "release", &format!("{ROOT}/gpasclientconf_live.php")).await?,
    )?;
    let release = field(&release, "id")?;
    ensure!(
        release
            .split('.')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())),
        "invalid GPAS release"
    );
    let bundle: Value = serde_json::from_str(
        &get(
            ctx,
            "bundle",
            &format!("{ROOT}/{release}/logical_bundle.php"),
        )
        .await?,
    )?;
    let path = field(&bundle["games"], code)?;
    ensure!(
        path.starts_with(&format!("/gpasdistributed/igv/{code}/"))
            && path.trim_end_matches('/').rsplit('/').next().is_some_and(
                |v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            ),
        "unexpected GPAS game path"
    );
    let manifest: Value = serde_json::from_str(
        &get(
            ctx,
            "manifest",
            &format!("{ROOT}{}/packs/manifest.json", path.trim_end_matches('/')),
        )
        .await?,
    )?;
    ensure!(manifest["gameCode"] == code, "GPAS game identity mismatch");
    let root = url::Url::parse(&format!("{ROOT}{}/", path.trim_end_matches('/')))?;
    let (sources, maps) = sources::load(ctx, &root, &manifest).await?;
    let contract = source_contract(&manifest, &sources, &binding(code))?;
    let env: Value = serde_json::from_str(
        &get(
            ctx,
            "environment",
            &format!("{ROOT}/{release}/brands/playtechhorizon/1/json/env_config.json"),
        )
        .await?,
    )?;
    let host = field(&env["server"], "host")?;
    ensure!(
        matches!(
            host,
            "gpas-stg.playtechone.com" | "frontend.playtechhorizon.com"
        ) && (env["server"]["secure"] == true || env["server"]["secure"] == "true"),
        "invalid GPAS demo server"
    );
    let handshake = get(
        ctx,
        "socket_handshake",
        &format!("https://{host}/socket.io/1/?t={}", now_ms()),
    )
    .await?;
    let parts: Vec<_> = handshake.trim().split(':').collect();
    if !(parts.len() == 4
        && parts[3].split(',').any(|v| v == "websocket")
        && !parts[0].is_empty()
        && parts[0]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
    {
        return Err(crate::model::unknown_event(
            "invalid GPAS WSS handshake".to_string(),
        ));
    };
    let mut socket = ctx
        .websocket(
            &format!("wss://{host}/socket.io/1/websocket/{}", parts[0]),
            ROOT,
            None,
            BTreeMap::new(),
        )
        .await?;
    ctx.ws_expect_text(&mut socket, "socket_ready", "1::")
        .await?;
    let identity = Uuid::new_v4().to_string();
    let mut features = vec!["freespinTotalBalance"];
    if manifest["setup"]["supportsFs2Bonus"] == true {
        features.push("freespin2");
    }
    let session = exchange(ctx,&mut socket,"session",json!({"_type":"InitGameSession","casino":"playtechhorizon",
        "clientType":"casino","clientPlatform":"web","language":"EN","gameCode":code,"username":identity,
        "token":identity,"resume":false,"apiFeatures":features,"real":0}),false,"InitGameSessionResponse").await?;
    ensure!(
        session["brokenGame"] == false,
        "GPAS rejected new demo session"
    );
    let state = exchange(
        ctx,
        &mut socket,
        "state",
        json!({"_type":"GetClientState"}),
        false,
        "GetClientStateResponse",
    )
    .await?;
    ensure!(
        state["gameCommand"].is_object() && state["gameCommand"]["gameRoundClosed"] != false,
        "GPAS demo has an open round"
    );
    let config = exchange(
        ctx,
        &mut socket,
        "configuration",
        json!({"_type":"com.pt.gpas.config.GetGameConfigurationRequest",
        "scoped":true,"disablePersonalizedBets":true}),
        false,
        "com.pt.gpas.config.GetGameConfigurationResponse",
    )
    .await?;
    let scopes = config["data"]
        .as_array()
        .context("GPAS configuration scopes missing")?;
    let offer = native::resolve_scoped(
        scopes,
        mode,
        &contract,
        &sources.join("\n"),
        &maps,
        settings,
    )?;
    ctx.save_session(&json!({"game":code,"owner":identity,"token":field(&session,"token")?,
        "source_sha256":sources.iter().map(|s|crate::model::sha256(s.as_bytes())).collect::<Vec<_>>(),
        "offer":offer.id,"selector":offer.selector,"multiplier":offer.multiplier}))?;
    let mut response = exchange(
        ctx,
        &mut socket,
        mode.kind.as_str(),
        entry(&offer, &contract)?,
        true,
        COMMAND,
    )
    .await?;
    let mut picks = Picks::default();
    // ctx also enforces the configured attempt limits. This bound prevents an
    // endless native loop even when a server keeps returning a recognized phase.
    for step in 0..ctx.max_steps().min(4096) {
        let decoded = native::decode_data(&response["gameData"])?;
        if step == 0 || decoded.get("stakeCostUuid").is_some() {
            ensure!(
                decoded["stakeCostUuid"] == offer.id,
                "GPAS native response cost identity mismatch"
            );
        }
        match next_step(&response, &contract, &mut picks, &settings["choices"])? {
            Step::Closed => {
                return Ok(Completion {
                    native_round_id: response["gameRoundId"].as_str().map(str::to_owned),
                    details: json!({"game":code,"offer":offer.id,"selector":offer.selector,"multiplier":offer.multiplier,"continuations":step}),
                });
            }
            Step::Continue(command) => {
                response =
                    exchange(ctx, &mut socket, "continuation", command, true, COMMAND).await?;
            }
        }
    }
    Err(crate::model::unknown_event(
        "GPAS continuation limit reached",
    ))
}

mod native;
mod sources;
#[cfg(test)]
mod tests;
