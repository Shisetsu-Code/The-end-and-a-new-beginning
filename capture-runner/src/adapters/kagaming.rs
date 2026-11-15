//! Native protocol reconstructed from standalone info/kagaming/codigo; no old runtime dependency.
//! Bonus Mania public demo, ported from the published KA client and receipt.
use crate::{context::RunContext, model::*};
use anyhow::{Context, Result, ensure};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::LazyLock, time::Duration};
use uuid::Uuid;

const ENGINE_SOURCE: &str = "https://gamesdemo.kaga88.com/game.min.2090.js";
const ENGINE_HASH: &str = "6cf1bdace2d5fb0a889364ad05fa3a3a31ed2fc37aa79b36be4ee71a6e49a0f2";

#[derive(Clone, Debug)]
struct Profile {
    game: String,
    sel: i64,
    cps: i64,
    dn: Value,
    atb: i64,
    reset_atb: bool,
    position: Option<Vec<u64>>,
    source_sha256: String,
}

#[derive(Deserialize)]
struct Binding {
    source_sha256: String,
    sel: i64,
    cps: i64,
    dn: Value,
    atb: i64,
    reset_atb: bool,
    position: Option<Vec<u64>>,
}

fn atb_for_step(atb: i64, reset_atb: bool, step: usize) -> i64 {
    if reset_atb && step > 0 { 0 } else { atb }
}

static BINDINGS: LazyLock<BTreeMap<String, BTreeMap<String, Binding>>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../bindings/kagaming.json"))
        .expect("valid embedded KA Gaming bindings")
});

fn binding(game: &Game, mode: &Mode) -> Option<&'static Binding> {
    let expected = if mode.key == "normal" {
        ModeKind::Normal
    } else if mode.key.starts_with("ante-") {
        ModeKind::AnteBet
    } else if mode.key.starts_with("buy-") {
        ModeKind::Purchase
    } else if game.id == "PenaltyShootout" && mode.key.starts_with("target-") {
        ModeKind::Variant
    } else {
        return None;
    };
    if mode.kind != expected {
        return None;
    }
    BINDINGS.get(&game.id)?.get(&mode.key)
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    binding(game, mode).is_some()
}
fn hash31(text: &str) -> i32 {
    text.encode_utf16().fold(0_i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    })
}
fn signed_context(body: &str, context: &Value, timestamp: u64) -> Result<Value> {
    let mut signed = context.as_object().context("KA context missing")?.clone();
    let device = context["idv"]
        .as_str()
        .context("KA device identity missing")?;
    let version = context["dv"]
        .as_str()
        .context("KA device version missing")?;
    let time = timestamp.to_string();
    // This constant is the published anonymous demo client's request signature,
    // never a credential loaded from another player's session.
    let salt = "SzXS@78TWls@x9&$5212Qw*oOZP@!tQybp5msFRpt1b@kaqN@3ms2932733tSyst";
    let e = hash31(&format!("{body}2090{device}{time}"));
    let f = hash31(&format!("{time}{device}{version}"));
    let source = if e % 2 == 0 {
        format!("{time}1.0.268 (2090){e}{salt}{f}{body}2090")
    } else {
        format!("{salt}{body}{f}{version}{e}2090{time}")
    };
    signed.insert(
        "ida".into(),
        json!(format!("{}{time}", sha256(source.as_bytes()))),
    );
    Ok(Value::Object(signed))
}
fn amount(value: &Value) -> Result<Decimal> {
    ensure!(
        value.is_string() || value.is_number(),
        "KA native number missing"
    );
    decimal(
        value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string())
            .as_str(),
    )
}
fn integer(value: &Value) -> Result<i64> {
    let n = amount(value)?;
    ensure!(n.fract().is_zero(), "KA native integer expected");
    n.normalize()
        .to_string()
        .parse()
        .context("KA native integer out of range")
}
fn populated(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Array(v)) => !v.is_empty(),
        Some(Value::Object(v)) => !v.is_empty(),
        Some(Value::String(v)) => !v.is_empty(),
        Some(Value::Number(v)) => v.to_string() != "0",
        _ => true,
    }
}
fn profile(init: &Value, game: &str, binding: &Binding) -> Result<Profile> {
    ensure!(
        init["gn"] == game && init["gm"] == 0 && init["mi"] == -1 && init["fs"] == false,
        "KA initialization is not a fresh demo for the selected game"
    );
    ensure!(
        init["e"] == false && init["ec"] == 0,
        "KA demo/currency not confirmed"
    );
    ensure!(
        !remaining(&init["sgr"]["lsd"])?,
        "KA fresh session has previous state"
    );
    let minimum = |key: &str| -> Result<i64> {
        let values = init[key]
            .as_array()
            .context("KA native offer missing")?
            .iter()
            .map(integer)
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            values.iter().all(|n| *n > 0),
            "KA native offer contains invalid amount"
        );
        values.into_iter().min().context("KA native offer empty")
    };
    let sel = minimum("ase")?;
    let cps = minimum("cup")?;
    let denominations = init["dn"].as_array().context("KA denominations missing")?;
    let mut dn: Option<(Decimal, Value)> = None;
    for value in denominations {
        let n = amount(value)?;
        ensure!(n > Decimal::ZERO, "KA invalid denomination");
        if dn.as_ref().is_none_or(|(old, _)| n < *old) {
            dn = Some((n, value.clone()));
        }
    }
    let dn = dn.context("KA denominations empty")?.1;
    ensure!(
        sel == binding.sel && cps == binding.cps && amount(&dn)? == amount(&binding.dn)?,
        "KA offer differs from the reviewed minimum"
    );
    Ok(Profile {
        game: game.into(),
        sel,
        cps,
        dn,
        atb: binding.atb,
        reset_atb: binding.reset_atb,
        position: binding.position.clone(),
        source_sha256: binding.source_sha256.clone(),
    })
}
fn free_spins_remaining(md: &Value) -> Result<bool> {
    if !(integer(&md["rf"])? == 0) {
        return Err(unknown_event("KA respin state requires a reviewed binding"));
    }
    let free_spins = integer(&md["fsr"])?;
    ensure!(free_spins >= 0, "KA negative free-spin count");
    ensure!(
        free_spins == 0 || !populated(md.get("fsd")),
        "KA bundled free spins require review"
    );
    Ok(free_spins > 0)
}

fn remaining(md: &Value) -> Result<bool> {
    if !(!populated(md.get("bpd"))) {
        return Err(unknown_event("KA bonus choice requires a reviewed binding"));
    }
    free_spins_remaining(md)
}

fn space_pick_pending(md: &Value) -> Result<bool> {
    let bonus = &md["bpd"];
    if bonus.is_null() {
        return Ok(false);
    }
    match (bonus["c"].as_bool(), bonus["s"].as_bool()) {
        (Some(true), Some(true)) => Ok(false),
        (Some(false), Some(false)) => {
            ensure!(
                md["acb"] == 2 && bonus["np"] == 3 && bonus["w"] == 0,
                "KA Space bonus offer changed"
            );
            Ok(true)
        }
        _ => Err(unknown_event("KA Space bonus flags changed")),
    }
}

impl Profile {
    fn position_for_step(&self, step: usize) -> Option<&Vec<u64>> {
        if step == 0 || self.game == "PenaltyShootout" {
            self.position.as_ref()
        } else {
            None
        }
    }

    fn pending(&self, md: &Value) -> Result<bool> {
        if self.game == "Space" {
            let free = free_spins_remaining(md)?;
            Ok(space_pick_pending(md)? || free)
        } else {
            remaining(md)
        }
    }
}
fn completed(profile: &Profile, steps: &[(Value, Value)]) -> Result<Option<Completion>> {
    let (_, response) = steps.last().context("KA native response missing")?;
    let md = &response["md"];
    ensure!(
        response["e"] == false && response["ec"] == 0 && md["v"] == true,
        "KA provider rejected native action"
    );
    let first = &steps[0].1["md"];
    let tid = first["tid"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("KA round id missing")?;
    ensure!(md["tid"] == tid, "KA round identity changed");
    if profile.pending(md)? {
        return Ok(None);
    }
    Ok(Some(Completion {
        native_round_id: Some(tid.into()),
        details: json!({"game":profile.game,"steps":steps.len(),"game_source_sha256":profile.source_sha256,"engine_source_sha256":ENGINE_HASH}),
    }))
}
pub fn validate(game: &Game, mode: &Mode, settings: &Value) -> Result<()> {
    ensure!(
        supports(game, mode),
        "KA mode has no reviewed native slot binding"
    );
    ensure!(
        settings.is_null() || settings.as_object().is_some_and(|o| o.is_empty()),
        "KA public binding has no overrides"
    );
    Ok(())
}
async fn get(ctx: &RunContext, label: &str, url: &str) -> Result<String> {
    let reply = ctx.get(label, url).await?;
    ensure!(reply.status == 200, "KA HTTP failure at {label}");
    Ok(reply.body)
}
async fn call(
    ctx: &RunContext,
    label: &str,
    path: &str,
    user: &str,
    context: &Value,
    body: &Value,
    round_action: bool,
) -> Result<Value> {
    let bytes = serde_json::to_string(body)?;
    let signed = signed_context(&bytes, context, now_ms())?;
    let headers = BTreeMap::from([
        ("content-type".into(), "application/json".into()),
        ("ctx".into(), serde_json::to_string(&signed)?),
        ("origin".into(), "https://gamesdemo.kaga88.com".into()),
        ("referer".into(), "https://gamesdemo.kaga88.com/".into()),
    ]);
    let url = format!("https://rmpdemo.kaga88.com{path}?ak=accessKey&cr=USD&m=0&u={user}");
    let reply = ctx
        .http(label, "POST", &url, headers, bytes, round_action)
        .await?;
    ensure!(reply.status == 200, "KA HTTP failure at {label}");
    let value: Value = serde_json::from_str(&reply.body).context("KA invalid native JSON")?;
    ensure!(
        value["e"] == false && value["ec"] == 0,
        "KA native command rejected at {label}"
    );
    Ok(value)
}
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    validate(game, mode, settings)?;
    let binding = binding(game, mode).context("KA native mode missing")?;
    let user = (100_000_000_u128 + Uuid::new_v4().as_u128() % 900_000_000).to_string();
    let launch = url::Url::parse_with_params(
        "https://gamesdemo.kaga88.com/",
        [
            ("g", game.id.as_str()),
            ("p", "demo"),
            ("u", user.as_str()),
            ("t", "1238"),
            ("ak", "accessKey"),
            ("cr", "USD"),
            ("loc", "es"),
        ],
    )?;
    get(ctx, "launcher", launch.as_str()).await?;
    let source_url = format!(
        "https://gamesdemo.kaga88.com/res/js/2090/{}.js",
        game.id.to_ascii_lowercase()
    );
    let source = get(ctx, "public_client", &source_url).await?;
    if sha256(source.as_bytes()) != binding.source_sha256 {
        return Err(unknown_event(
            "KA game client serialization version changed",
        ));
    }
    let engine = get(ctx, "public_engine", ENGINE_SOURCE).await?;
    if sha256(engine.as_bytes()) != ENGINE_HASH {
        return Err(unknown_event(
            "KA public engine serialization version changed",
        ));
    }
    let mut context = json!({"dt":"Evidencia Rust QA client","dv":env!("CARGO_PKG_VERSION"),"av":"1.0.268 (2090)","idv":sha256(Uuid::new_v4().as_bytes()),"lg":"es","do":"Linux","as":"","ak":"accessKey"});
    let start = json!({"un":user,"pn":"demo","ak":"accessKey","gn":game.id,"loc":"es","to":"123","cr":"USD","gm":0,"tb":null,"mi":-1,"mc":0});
    let init = call(
        ctx,
        "init",
        "/kaga/rmp/startGame",
        &user,
        &context,
        &start,
        false,
    )
    .await?;
    let session = init["si"]
        .as_str()
        .filter(|s| !s.is_empty() && *s != "[REDACTED]")
        .context("KA fresh demo session missing")?;
    let returned_user = init["un"]
        .as_str()
        .filter(|s| !s.is_empty() && *s != "[REDACTED]")
        .context("KA fresh demo user missing")?;
    // The server returns the canonical anonymous account used in ctx.u; the
    // published client keeps the original public user in the query string.
    context["u"] = json!(returned_user);
    context["c"] = json!(session);
    ctx.save_session(&json!({"provider":"kagaming","game":game.id,"demo_user":user,"session_id":session,"context":context,"currency":"USD","game_source_sha256":binding.source_sha256,"engine_source_sha256":ENGINE_HASH}))?;
    let p = profile(&init, &game.id, binding)?;
    let mut steps: Vec<(Value, Value)> = Vec::new();
    let seed = now_ms() % 2_000_000_000;
    for index in 0..ctx.max_steps() {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if p.game == "Space"
            && let Some((_, previous)) = steps.last()
            && space_pick_pending(&previous["md"])?
        {
            let request = json!({"gn":game.id,"p":0,"sid":integer(&previous["md"]["sid"])?});
            let reply = call(
                ctx,
                "bonus_pick",
                "/kaga/command/bonusPick",
                &user,
                &context,
                &request,
                true,
            )
            .await?;
            steps.push((request, reply));
            if let Some(done) = completed(&p, &steps)? {
                return Ok(done);
            }
            continue;
        }
        let mut request = json!({"gn":game.id,"sel":p.sel,"sid":seed+index as u64+1,
            "cps":p.cps,"atb":atb_for_step(p.atb, p.reset_atb, index),"dn":p.dn});
        if let Some(position) = p.position_for_step(index) {
            request["pos"] = json!(position);
        }
        let reply = call(
            ctx,
            if index == 0 {
                mode.kind.operation()
            } else {
                "continuation"
            },
            "/kaga/command/spin",
            &user,
            &context,
            &request,
            true,
        )
        .await?;
        steps.push((request, reply));
        if let Some(done) = completed(&p, &steps)? {
            return Ok(done);
        }
    }
    anyhow::bail!("KA continuation limit reached")
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    fn fixture() -> Profile {
        Profile {
            game: "BonusMania".into(),
            sel: 1,
            cps: 1,
            dn: json!(1),
            atb: 0,
            reset_atb: false,
            position: None,
            source_sha256: "fixture".into(),
        }
    }
    #[test]
    fn terminal_native_spin_needs_no_bl_cr_pcr_or_awards() {
        let steps = vec![(
            json!({}),
            json!({"e":false,"ec":0,"md":{"v":true,"tid":"r","rf":0,"fsr":0,"bpd":null}}),
        )];
        assert_eq!(
            completed(&fixture(), &steps)
                .unwrap()
                .unwrap()
                .native_round_id
                .as_deref(),
            Some("r")
        );
    }
    #[test]
    fn new_native_bonus_is_typed() {
        let err = fixture()
            .pending(&json!({"rf":0,"fsr":0,"bpd":{"new":true}}))
            .unwrap_err();
        assert!(err.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn signature_preserves_original_request_bytes() {
        let a = signed_context("{\"a\":1}", &json!({"idv":"device","dv":"v"}), 7).unwrap();
        let b = signed_context("{\"a\":2}", &json!({"idv":"device","dv":"v"}), 7).unwrap();
        assert_ne!(a["ida"], b["ida"]);
        assert_eq!(a["idv"], "device");
    }
}
