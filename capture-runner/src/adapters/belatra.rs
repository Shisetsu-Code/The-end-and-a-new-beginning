//! Native protocol reconstructed from standalone info/belatra/codigo; no old runtime dependency.
use super::belatra_crypto;
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, now_ms, unknown_event},
};
use anyhow::{Context, Result, ensure};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::Url;
use uuid::Uuid;

const ORIGIN: &str = "https://demo.bltrm.com";

fn amount(v: &Value) -> Result<Decimal> {
    ensure!(v.is_number() || v.is_string(), "missing native amount");
    let value = decimal(
        &v.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string()),
    )?;
    ensure!(value >= Decimal::ZERO, "negative native amount");
    Ok(value)
}

fn minimum(v: &Value) -> Result<Value> {
    let mut least = None;
    for value in v.as_array().context("missing native menu")? {
        let n = amount(value)?;
        ensure!(
            n > Decimal::ZERO && value.is_number(),
            "invalid native menu value"
        );
        if least.as_ref().is_none_or(|(prior, _)| n < *prior) {
            least = Some((n, value.clone()));
        }
    }
    Ok(least.context("empty native menu")?.1)
}

struct Session {
    sid: String,
    secret: String,
    document: Url,
    modification: Value,
    uid: String,
    counter: u32,
}
impl Session {
    async fn exchange(
        &mut self,
        ctx: &RunContext,
        mut command: Value,
        round_action: bool,
    ) -> Result<Value> {
        command["uid"] = json!(self.uid);
        command["c"] = json!(self.counter % 1000);
        command["modification"] = self.modification.clone();
        self.counter += 1;
        let now = now_ms();
        let mut prefix = [0; 8];
        prefix[..2].copy_from_slice(&((now % 1000) as u16).to_le_bytes());
        prefix[2..4].copy_from_slice(&Uuid::new_v4().as_bytes()[..2]);
        prefix[4..].copy_from_slice(&((now / 1000) as u32).to_le_bytes());
        ctx.note("native_request", &json!({"request":command}))?;
        let payload = belatra_crypto::seal(&serde_json::to_vec(&command)?, &self.secret, prefix);
        let form = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("d", &payload)
            .append_pair("sid", &self.sid)
            .finish();
        let headers = BTreeMap::from([
            (
                "content-type".into(),
                "application/x-www-form-urlencoded; charset=UTF-8".into(),
            ),
            ("x-requested-with".into(), "XMLHttpRequest".into()),
            ("accept".into(), "*/*".into()),
            ("origin".into(), ORIGIN.into()),
            ("referer".into(), self.document.to_string()),
        ]);
        let label = command["q"].as_str().context("missing operation")?;
        let response = ctx
            .http(
                label,
                "POST",
                &format!("{ORIGIN}/game"),
                headers,
                form,
                round_action,
            )
            .await?;
        ensure!(
            response.status == 200,
            "Belatra native HTTP {}",
            response.status
        );
        let decoded = belatra_crypto::open(serde_json::from_str(&response.body)?, &self.secret)?;
        ctx.note("native_response", &json!({"response":decoded}))?;
        Ok(decoded)
    }
}

async fn launch(ctx: &RunContext, game: &str) -> Result<Session> {
    ensure!(
        !game.is_empty()
            && game
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'),
        "invalid game id"
    );
    let referer = BTreeMap::from([("referer".into(), "https://belatragames.com/".into())]);
    let url = Url::parse(&format!("{ORIGIN}/belatra/demo?game={game}&language=es"))?;
    let response = ctx
        .http(
            "demo_launch",
            "GET",
            url.as_str(),
            referer.clone(),
            String::new(),
            false,
        )
        .await?;
    ensure!(
        response.status == 302,
        "official demo did not issue a redirect"
    );
    let document = url.join(
        response
            .headers
            .get("location")
            .context("demo redirect missing")?,
    )?;
    ensure!(
        document.origin().ascii_serialization() == ORIGIN && document.path() == "/",
        "unreviewed launcher redirect"
    );
    let sid = document
        .query_pairs()
        .find(|(key, _)| key == "sid")
        .context("demo session not issued")?
        .1
        .into_owned();
    let modification = document
        .query_pairs()
        .find(|(key, _)| key == "modification")
        .context("game modification missing")?
        .1
        .into_owned();
    let response = ctx
        .http(
            "demo_document",
            "GET",
            document.as_str(),
            referer,
            String::new(),
            false,
        )
        .await?;
    ensure!(response.status == 200, "official demo document unavailable");
    let (_, source) = response
        .body
        .split_once("var config = ")
        .context("native config missing")?;
    let config = Value::deserialize(&mut serde_json::Deserializer::from_str(source.trim_start()))?;
    ensure!(
        config["nickname"] == game
            && config["request_crypt"] == true
            && config["user"]["sid"] == sid
            && (config["modification"].as_str() == Some(&modification)
                || config["modification"].is_number() && config["modification"] == modification),
        "native launch identity mismatch"
    );
    let secret = config["sc"]
        .as_str()
        .filter(|x| !x.is_empty())
        .context("native session key missing")?
        .to_owned();
    let uid = format!("_{}", &Uuid::new_v4().simple().to_string()[..9]);
    ctx.save_session(
        &json!({"sid":sid,"sc":secret,"uid":uid,"game":game,"modification":config["modification"]}),
    )?;
    Ok(Session {
        sid,
        secret,
        document,
        modification: config["modification"].clone(),
        uid,
        counter: 0,
    })
}

fn select_mode(gs: &Value, mode: &str, start: &mut Value) -> Result<Decimal> {
    match mode {
        "NORMAL" => Ok(Decimal::ONE),
        "VIP" => {
            let multiplier = amount(&gs["vipMode"]["vipBetK"])?;
            ensure!(multiplier > Decimal::ZERO, "VIP not advertised");
            start["vipOn"] = json!(1);
            Ok(multiplier)
        }
        _ => {
            let index = mode
                .strip_prefix("PURCHASE_")
                .context("unknown mode")?
                .parse::<usize>()?;
            let offer = &gs["buyBonus"]["buyTotalBetK"];
            let factor = if let Some(options) = offer.as_array() {
                let option = options.get(index).context("purchase not advertised")?;
                start["selectId"] = json!(index);
                amount(&option["cost"])?
            } else {
                ensure!(index == 0 && offer.is_number(), "purchase not advertised");
                amount(offer)?
            };
            ensure!(factor >= Decimal::ONE, "invalid native purchase price");
            start["buyBonus"] = json!(1);
            Ok(factor)
        }
    }
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    let mut session = launch(ctx, &game.id).await?;
    let enter = session
        .exchange(
            ctx,
            json!({"q":"enter","curFloor":1,"userAgent":ctx.user_agent()}),
            false,
        )
        .await?;
    let gs = &enter["gs"];
    ensure!(
        gs["phaseNext"] == "toIdle",
        "idle fictional FUN demo not confirmed"
    );

    let lines = gs["nlines"].clone();
    ensure!(
        gs["linesAssortment"]
            .as_array()
            .is_some_and(|a| a.contains(&lines)),
        "unreviewed line menu"
    );
    let bets = gs["betDependOnLines"]
        .as_array()
        .and_then(|a| a.iter().find(|row| row["lines"] == lines))
        .map_or(&gs["betAssortment"], |row| &row["betAssort"]);
    let bet = minimum(bets)?;
    let denom = minimum(&gs["denomAssortment_cents"])?;
    let mut start = json!({"q":"start","betPerLine":bet,"nlines":lines,"denom":denom,
        "buyBonus":null,"selectId":null,"vipOn":0,"hideInsideInHistory":0,"showingInMoney":0});
    select_mode(gs, &mode.key, &mut start)?;
    if let Some(id) = gs.get("historyId") {
        start["ghistId"] = id.clone();
    }
    let mut state = session.exchange(ctx, start, true).await?;
    let history = state["gs"]["historyId"].clone();
    ensure!(
        history.is_string() || history.is_number(),
        "native stake or round identity mismatch"
    );
    for _ in 0..ctx.max_steps() {
        ensure!(
            state["gs"].get("historyId").is_none_or(|id| *id == history),
            "Belatra round owner changed"
        );
        let gs = &state["gs"];
        let Some(request) = next_request(gs, &history)? else {
            let final_state = session
                .exchange(ctx, json!({"q":"finish","ghistId":history}), true)
                .await?;
            validate_finish(&final_state["gs"], &history)?;
            return Ok(Completion {
                native_round_id: Some(
                    history
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| history.to_string()),
                ),
                details: json!({"game":game.id}),
            });
        };
        state = session.exchange(ctx, request, true).await?;
    }
    anyhow::bail!("native continuation limit reached")
}

pub fn supports(_game: &Game, mode: &Mode) -> bool {
    match mode.kind {
        ModeKind::Normal => mode.key == "NORMAL",
        ModeKind::AnteBet => mode.key == "VIP",
        ModeKind::Purchase => mode
            .key
            .strip_prefix("PURCHASE_")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())),
        _ => false,
    }
}

fn next_request(gs: &Value, history: &Value) -> Result<Option<Value>> {
    let next = gs["phaseNext"]
        .as_str()
        .ok_or_else(|| unknown_event("Belatra phase missing"))?;
    match next {
        "toPaid" | "toDoubleDialog" => Ok(None),
        "toRespin" | "toNextFG" if gs["respinInfo"]["nIt"].as_u64().is_some_and(|n| n >= 1) => Ok(
            Some(json!({"q":"play","userAction":"askRespin","ghistId":history})),
        ),
        "toNextFG" if gs["freeInfo"]["remain"].as_u64().is_some_and(|n| n >= 1) => Ok(Some(
            json!({"q":"play","userAction":"askNextFG","remainFG":gs["freeInfo"]["remain"],"ghistId":history}),
        )),
        _ => Err(unknown_event(format!(
            "Belatra unrecognized next phase: {next}"
        ))),
    }
}
fn validate_finish(gs: &Value, history: &Value) -> Result<()> {
    if gs["phaseCur"] != "finished" || gs["phaseNext"] != "toIdle" {
        return Err(unknown_event("Belatra native finish did not close"));
    }
    ensure!(
        gs.get("historyId").is_none_or(|id| id == history),
        "Belatra finish round changed"
    );
    Ok(())
}
#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn terminal_phase_and_finish_ignore_wallet_and_win() {
        assert!(
            next_request(&json!({"phaseNext":"toPaid"}), &json!("r"))
                .unwrap()
                .is_none()
        );
        validate_finish(
            &json!({"phaseCur":"finished","phaseNext":"toIdle","historyId":"r"}),
            &json!("r"),
        )
        .unwrap();
    }
    #[test]
    fn native_free_continuation_carries_original_history() {
        let request = next_request(
            &json!({"phaseNext":"toNextFG","freeInfo":{"remain":3}}),
            &json!("r"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(request["userAction"], "askNextFG");
        assert_eq!(request["ghistId"], "r");
        assert_eq!(request["remainFG"], 3);
    }
    #[test]
    fn unknown_phase_is_typed_novelty() {
        assert!(
            next_request(&json!({"phaseNext":"new"}), &json!("r"))
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
