//! African Cats native decoder plus the explicit HTTP controls recovered from capture.py.
//! Additional games preserve binary replies opaquely and follow the native HTTP envelope.
#[path = "rubyplay_extra.rs"]
mod extra;
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, sha256, unknown_event},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::read::GzDecoder;
use rquickjs::{CatchResultExt, Context as JsContext, Runtime};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    time::{Duration, Instant},
};
use url::Url;

const GAME: &str = "rp_47";
const MATH: u64 = 20201103;
const API: &str = "https://srv.prrpeu3.com";
const SOURCE: &str =
    "https://static2.eu3cdn-83.servpavloro.com/africancats/latest/static/js/game.js";
const HASH: &str = "cd94725c10fbd5a1c70313b64c5eacaa6d17c89d52ea745601c48de7133c9c50";

pub fn supports(game: &Game, mode: &Mode) -> bool {
    original_supports(game, mode) || extra::supports(game, mode)
}

fn original_supports(game: &Game, mode: &Mode) -> bool {
    game.id == GAME && mode.kind == ModeKind::Normal && mode.key == "normal"
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Fragment {
    Script(String),
    Slice(String, usize, usize, String),
}

impl Fragment {
    fn append(&self, source: &str, out: &mut String) -> Result<()> {
        match self {
            Self::Script(s) => out.push_str(s),
            Self::Slice(prefix, at, len, suffix) => {
                out.push_str(prefix);
                out.push_str(
                    source
                        .get(
                            *at..at
                                .checked_add(*len)
                                .context("RubyPlay source range overflow")?,
                        )
                        .context("RubyPlay source range outside bundle")?,
                );
                out.push_str(suffix);
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Extraction {
    source_sha256: String,
    definitions: Vec<Fragment>,
    calls: Vec<Vec<Fragment>>,
}

fn inspect(source: &str, init: &Value, replies: &[(u64, Value)]) -> Result<Value> {
    let extraction: Extraction = serde_json::from_str(include_str!(
        "../../bindings/rubyplay-rp47-source-index.json"
    ))?;
    if extraction.source_sha256 != HASH || sha256(source.as_bytes()) != HASH {
        return Err(unknown_event("RubyPlay native bundle changed"));
    }
    let runtime = Runtime::new()?;
    runtime.set_memory_limit(96 * 1024 * 1024);
    runtime.set_max_stack_size(1024 * 1024);
    let deadline = Instant::now() + Duration::from_secs(5);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let context = JsContext::full(&runtime)?;
    context.with(|ctx| -> Result<Value> {
        let run = |script: &str| -> Result<()> {
            ctx.eval::<(), _>(script).catch(&ctx).map_err(|e| anyhow::anyhow!("RubyPlay decoder: {e}"))
        };
        run("Math.random=()=>{throw Error('native random fallback forbidden')};var console={log(){},warn(){},error(){}};")?;
        let mut definitions = String::new();
        for f in extraction.definitions { f.append(source, &mut definitions)?; definitions.push('\n'); }
        run(&definitions)?;
        for call in extraction.calls {
            let mut script = String::new();
            for f in call { f.append(source, &mut script)?; }
            run(&script)?;
        }
        run("var nativeProxy=CE.createProxy();if(_t.getProtocolVersion()!==2||nativeProxy.getEngine().getMathVersion()!==20201103)throw Error('unexpected native decoder version');")?;
        let initial = binary(init)?;
        run(&format!("var initial=new Js(0);initial.addField('binary',{initial});"))?;
        // Pass optional player data to the native decoder, without requiring or reconciling it.
        if let Some(balance)=init.pointer("/data/player/balance").filter(|v|v.is_number()) {run(&format!("initial.addField('balance',{balance});"))?;}
        run("nativeProxy.handleResponse(initial);")?;
        let bets = init["data"]["game_config"]["bets"].as_array().context("RubyPlay bet menu missing")?;
        let min = bets.iter().map(amount).collect::<Result<Vec<_>>>()?.into_iter().min().filter(|n| *n > 0)
            .context("RubyPlay has no positive minimum bet")?;
        let mut phases = Vec::new();
        for (action, reply) in replies {
            run(&format!("var request=new Js({action});if({action}===1)request.addField('bet',{min});nativeProxy.prepareRequest(request);"))?;
            let bytes = binary(reply)?;
            let an = amount(&reply["data"]["an"])?;
            run(&format!("var message=new Js({action});message.addField('binary',{bytes}).addField('an',{an});"))?;
            if let Some(balance)=reply.pointer("/data/player/balance").filter(|v|v.is_number()) {run(&format!("message.addField('balance',{balance});"))?;}
            run("nativeProxy.handleResponse(message);")?;
            let phase: String = ctx.eval("JSON.stringify({next:nativeProxy.getSession().nextAction()})")
                .catch(&ctx).map_err(|e| anyhow::anyhow!("RubyPlay result decoder: {e}"))?;
            phases.push(serde_json::from_str::<Value>(&phase)?);
        }
        let request: String = ctx.eval(format!("JSON.stringify((()=>{{var action=nativeProxy.getSession().nextAction(),request=new Js(action);if(action===1)request.addField('bet',{min});nativeProxy.prepareRequest(request);return {{action,fields:request.getFields()}}}})())"))
            .catch(&ctx).map_err(|e| anyhow::anyhow!("RubyPlay request decoder: {e}"))?;
        Ok(json!({"next":serde_json::from_str::<Value>(&request)?,"phases":phases}))
    })
}

fn binary(reply: &Value) -> Result<String> {
    let encoded = reply["data"]["binary"]
        .as_str()
        .context("RubyPlay binary missing")?;
    ensure!(encoded.len() <= 256 * 1024, "RubyPlay binary too large");
    Ok(serde_json::to_string(&STANDARD.decode(encoded)?)?)
}

fn amount(value: &Value) -> Result<u64> {
    value
        .as_u64()
        .filter(|n| *n <= 9_007_199_254_740_991)
        .context("RubyPlay integer missing or outside exact range")
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    if extra::supports(game, mode) {
        return extra::execute(ctx, game, mode).await;
    }
    ensure!(
        original_supports(game, mode),
        "RubyPlay mode has no reviewed binding"
    );
    let headers = BTreeMap::from([
        ("user-agent".into(), "Mozilla/5.0".into()),
        (
            "accept".into(),
            "text/html,application/json,application/javascript".into(),
        ),
    ]);
    let source = ctx
        .http(
            "native_source",
            "GET",
            SOURCE,
            headers.clone(),
            String::new(),
            false,
        )
        .await?;
    ensure!(
        source.status == 200,
        "RubyPlay source HTTP {}",
        source.status
    );
    let content = if source
        .headers
        .get("x-evidencia-body-encoding")
        .is_some_and(|s| s == "base64")
    {
        STANDARD.decode(source.body)?
    } else {
        source.body.into_bytes()
    };
    let source = if content.starts_with(&[0x1f, 0x8b]) {
        let mut value = String::new();
        GzDecoder::new(content.as_slice())
            .take(4 * 1024 * 1024)
            .read_to_string(&mut value)?;
        value
    } else {
        String::from_utf8(content)?
    };
    if sha256(source.as_bytes()) != HASH {
        return Err(unknown_event("RubyPlay native bundle changed"));
    }
    let issuer = Url::parse_with_params(
        &format!("{API}/init-session/demo"),
        [
            ("gamename", GAME),
            ("operator", "rubyplay.com"),
            ("mode", "fun"),
            ("currency", "EUR"),
            ("playerSession", ""),
        ],
    )?;
    let session = ctx
        .http(
            "demo_session",
            "GET",
            issuer.as_str(),
            headers.clone(),
            String::new(),
            false,
        )
        .await?;
    ensure!(
        session.status == 200,
        "RubyPlay issuer HTTP {}",
        session.status
    );
    let session: Value = serde_json::from_str(&session.body)?;
    let key = session["sessionKey"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("RubyPlay session missing")?;
    ensure!(
        session["status"] == "success" && !session["funModeData"].is_null(),
        "RubyPlay demo unavailable"
    );
    let endpoint = format!("{API}/gameserver/demo");
    let init = ctx
        .json(
            "init",
            &endpoint,
            &json!({"v_protocol":2,"v_math":MATH,"key":key,"device_type":"desktop",
        "funModeData":session["funModeData"],"action":"init"}),
            headers.clone(),
            false,
        )
        .await?;
    ensure!(
        init["status"] == "ok"
            && init["topic"] == "gameserver/init"
            && init["data"]["next_action"] == "spin"
            && init["data"]["an"] == 0,
        "RubyPlay demo is not idle"
    );
    let mut fun = init["funModeData"].clone();
    let mut replies = Vec::new();
    ctx.save_session(&json!({"game":GAME,"key":key}))?;
    let mut decoded = inspect(&source, &init, &replies)
        .map_err(|e| unknown_event(format!("RubyPlay decoder: {e:#}")))?;
    for index in 0..ctx.max_steps() {
        let next = &decoded["next"];
        let action = amount(&next["action"])?;
        let name = match (index, action) {
            (0, 1) => "spin",
            (1.., 2) => "freespin",
            (1.., 3) => "respin",
            _ => {
                return Err(unknown_event(format!(
                    "RubyPlay unknown continuation action {action}"
                )));
            }
        };
        ensure!(!fun.is_null(), "RubyPlay session payload missing");
        let mut wire = next["fields"].clone();
        if !wire.is_object()
            || wire["v_protocol"] != 2
            || wire["v_math"] != MATH
            || wire["an"] != index as u64
        {
            return Err(unknown_event("RubyPlay native request binding changed"));
        }
        wire["key"] = json!(key);
        wire["device_type"] = json!("desktop");
        wire["funModeData"] = fun;
        wire["action"] = json!(name);
        let reply = ctx
            .json(name, &endpoint, &wire, headers.clone(), true)
            .await?;
        ensure!(reply["status"] == "ok", "RubyPlay provider rejected action");
        if reply["data"]["an"] != (index + 1) as u64 {
            return Err(unknown_event("RubyPlay action sequence changed"));
        }
        fun = reply["funModeData"].clone();
        replies.push((action, reply));
        decoded = inspect(&source, &init, &replies)
            .map_err(|e| unknown_event(format!("RubyPlay decoder: {e:#}")))?;
        if decoded["phases"][index]["next"] == 1 {
            if replies[index].1["data"]["next_action"] != "spin" {
                return Err(unknown_event("RubyPlay terminal marker changed"));
            }
            return Ok(Completion {
                native_round_id: None,
                details: json!({"game":GAME,"actions":index+1,"source_sha256":HASH}),
            });
        }
    }
    Err(unknown_event("RubyPlay continuation limit reached"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_http_controls_extend_only_the_documented_catalogue_modes() {
        for id in [
            "rp_1", "rp_102", "rp_103", "rp_106", "rp_109", "rp_112", "rp_114", "rp_222",
        ] {
            let game = Game {
                id: id.into(),
                name: String::new(),
                modes: vec![],
            };
            let mode = Mode {
                key: "normal".into(),
                kind: ModeKind::Normal,
                label: String::new(),
                requires_prerequisites: Value::Null,
            };
            assert!(supports(&game, &mode), "missing source-known normal: {id}");
            let buy = Mode {
                key: "buy_feature:freespin".into(),
                kind: ModeKind::Purchase,
                ..mode
            };
            assert_eq!(supports(&game, &buy), matches!(id, "rp_102" | "rp_103"));
        }
    }
    #[test]
    fn changed_decoder_source_is_typed_novelty() {
        let err = inspect("changed source", &json!({}), &[]).unwrap_err();
        assert!(err.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn binary_payload_needs_no_balance() {
        assert_eq!(
            binary(&json!({"data":{"binary":"AAEC"}})).unwrap(),
            "[0,1,2]"
        );
    }
}
