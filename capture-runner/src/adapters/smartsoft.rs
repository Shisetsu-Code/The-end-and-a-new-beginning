//! SmartSoft's published SlotsDemo option protocol. Only native selectors are interpreted.
use crate::{
    context::{RunContext, WireResponse},
    model::{Completion, Game, Mode, ModeKind, decimal, exact_mul, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use url::Url;

pub fn supports(game: &Game, mode: &Mode) -> bool {
    !game.id.is_empty()
        && game
            .id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && selection(mode).is_ok()
}
fn selection(mode: &Mode) -> Result<(&str, Option<u64>)> {
    if mode.kind == ModeKind::Normal && mode.key == "normal" {
        return Ok(("Start", None));
    }
    let (option, index) = mode
        .key
        .split_once(':')
        .context("SmartSoft mode needs an explicit native option index")?;
    ensure!(
        matches!(
            (option, mode.kind),
            ("PurchaseBonus", ModeKind::Purchase) | ("RiskBet", ModeKind::AnteBet)
        ),
        "unsupported SmartSoft mode"
    );
    Ok((
        option,
        Some(index.parse().context("invalid SmartSoft option index")?),
    ))
}
fn decode_entities(text: &str) -> String {
    let entities =
        Regex::new(r"&(?:amp|quot|apos|lt|gt|#(?:x[0-9a-fA-F]+|[0-9]+));").expect("static regex");
    entities
        .replace_all(text, |c: &regex::Captures<'_>| match &c[0] {
            "&amp;" => "&".into(),
            "&quot;" => "\"".into(),
            "&apos;" => "'".into(),
            "&lt;" => "<".into(),
            "&gt;" => ">".into(),
            e => {
                let digits = &e[2..e.len() - 1];
                let code = if let Some(h) = digits.strip_prefix('x') {
                    u32::from_str_radix(h, 16)
                } else {
                    digits.parse()
                };
                code.ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| e.into())
            }
        })
        .into_owned()
}
fn document_fields(document: &str) -> HashMap<String, String> {
    let inputs = Regex::new(r"(?i)<input\b[^>]*>").expect("static regex");
    let attributes = Regex::new(r#"([a-zA-Z_][\w:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#)
        .expect("static regex");
    let mut fields = HashMap::new();
    for input in inputs.find_iter(document) {
        let attrs: HashMap<_, _> = attributes
            .captures_iter(input.as_str())
            .map(|c| {
                let v = c
                    .get(2)
                    .or_else(|| c.get(3))
                    .or_else(|| c.get(4))
                    .expect("matched value");
                (c[1].to_ascii_lowercase(), decode_entities(v.as_str()))
            })
            .collect();
        if let Some(id) = attrs.get("id") {
            fields.insert(id.clone(), attrs.get("value").cloned().unwrap_or_default());
        }
    }
    fields
}
fn checked(response: WireResponse) -> Result<WireResponse> {
    ensure!(response.status == 200, "SmartSoft HTTP {}", response.status);
    Ok(response)
}
fn decode(response: WireResponse) -> Result<Value> {
    Ok(serde_json::from_str(&checked(response)?.body)?)
}
struct Demo {
    loader: Url,
    game_id: String,
    token: String,
    fraction_digits: u32,
}
impl Demo {
    fn endpoint(&self, method: &str, tail: Option<&str>) -> Result<Url> {
        let mut u = self.loader.join("/SlotsDemo//api/GenericSlotApi")?;
        {
            let mut p = u
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("invalid SmartSoft API path"))?;
            p.extend(method.split('/'))
                .push(&self.game_id)
                .push(&self.token);
            if let Some(tail) = tail {
                p.push(tail);
            }
        }
        Ok(u)
    }
    fn headers(&self) -> BTreeMap<String, String> {
        BTreeMap::from([("Referer".into(), self.loader.to_string())])
    }
}
fn validate_loader(loader: &Url) -> Result<()> {
    let shard = Regex::new(r"^eu-server-w[0-9]+\.ssgportal\.com$").expect("static regex");
    ensure!(
        loader.scheme() == "https"
            && loader.host_str().is_some_and(|h| shard.is_match(h))
            && loader.path().starts_with("/SlotsDemo/")
            && loader.username().is_empty()
            && loader.password().is_none()
            && loader.port().is_none(),
        "SmartSoft redirect leaves the official SlotsDemo path"
    );
    Ok(())
}
async fn open(ctx: &RunContext, game: &str) -> Result<Demo> {
    let catalogue = checked(
        ctx.get(
            "smartsoft_catalogue",
            "https://www.smartsoftgaming.com/games",
        )
        .await?,
    )?
    .body;
    let href = Regex::new(r#"href="([^"]+)""#).expect("static regex");
    let entry = href
        .captures_iter(&catalogue)
        .find_map(|c| {
            let s = decode_entities(&c[1]);
            let s = if s.starts_with("eu-server.ssgportal.com/") {
                format!("https://{s}")
            } else {
                s
            };
            let mut u = Url::parse(&s).ok()?;
            if !matches!(u.scheme(), "http" | "https")
                || u.host_str() != Some("eu-server.ssgportal.com")
                || u.path() != "/GameLauncher/Loader.aspx"
                || !u.username().is_empty()
                || u.password().is_some()
                || u.port().is_some()
            {
                return None;
            }
            if !u
                .query_pairs()
                .any(|(k, v)| k == "PortalName" && v.eq_ignore_ascii_case("DEMO"))
                || !u.query_pairs().any(|(k, v)| k == "GameName" && v == game)
            {
                return None;
            }
            u.set_scheme("https").ok()?;
            Some(u)
        })
        .context("SmartSoft game has no official demo launcher")?;
    let launch = checked(ctx.get("smartsoft_launcher", entry.as_str()).await?)?.body;
    let pattern = Regex::new(
        r#"https://eu-server-w[0-9]+\.ssgportal\.com/SlotsDemo/Loader\.aspx\?[^\s"'<>]+"#,
    )
    .expect("static regex");
    let mut loader = Url::parse(&decode_entities(
        pattern
            .find(&launch)
            .ok_or_else(|| unknown_event("SmartSoft SlotsDemo loader missing"))?
            .as_str(),
    ))?;
    let mut document = None;
    for _ in 0..4 {
        validate_loader(&loader)?;
        let r = ctx.get("smartsoft_demo_document", loader.as_str()).await?;
        match r.status {
            200 => {
                document = Some(r.body);
                break;
            }
            301 | 302 | 303 | 307 | 308 => {
                loader = loader.join(
                    r.headers
                        .get("location")
                        .context("SmartSoft redirect lacks Location")?,
                )?;
            }
            status => bail!("SmartSoft demo document HTTP {status}"),
        }
    }
    let mut fields = document_fields(&document.context("SmartSoft redirect limit reached")?);
    ensure!(
        fields
            .get("PortalName")
            .is_some_and(|s| s.eq_ignore_ascii_case("DEMO")),
        "SmartSoft launcher did not confirm DEMO portal"
    );
    let fraction_digits = serde_json::from_str::<Value>(
        fields
            .get("Currency")
            .context("SmartSoft request precision descriptor missing")?,
    )?["FractionDigit"]
        .as_u64()
        .filter(|n| *n <= 8)
        .ok_or_else(|| unknown_event("SmartSoft request precision changed"))?
        as u32;
    let game_id = fields
        .remove("GameId")
        .filter(|s| !s.is_empty())
        .context("SmartSoft GameId missing")?;
    let token = fields
        .remove("Token")
        .filter(|s| !s.is_empty())
        .context("SmartSoft session token missing")?;
    ctx.save_session(&json!({"loader":loader.as_str(),"game_id":game_id,"token":token}))?;
    Ok(Demo {
        loader,
        game_id,
        token,
        fraction_digits,
    })
}
fn board(response: &Value) -> &Value {
    response["Board"]["MachineInfo"]
        .as_array()
        .and_then(|a| a.last())
        .unwrap_or(&response["PlayerEvent"]["Board"])
}
fn offered(response: &Value, option: &str) -> bool {
    response["PlayerEvent"]["Options"]
        .as_array()
        .is_some_and(|a| a.iter().any(|o| o["OptionKey"] == option))
}
fn selector(response: &Value, digits: u32) -> Result<String> {
    let board = board(response);
    let amount = |key: &str| -> Result<rust_decimal::Decimal> {
        let v = &board[key];
        decimal(v.as_str().unwrap_or(&v.to_string()))
    };
    // These values are native SelectedPieces encoding, never balance/cost acceptance.
    Ok(format!(
        "{},{},{}",
        amount("LinePlay")?.normalize(),
        amount("BetPerLine")?.normalize(),
        exact_mul(
            amount("ChipCoef")?,
            decimal(&10u64.pow(digits).to_string())?
        )?
        .normalize()
    ))
}
fn continuation(response: &Value) -> Result<Option<&'static str>> {
    let name = response["PlayerEvent"]["EventType"]["Code"]
        .as_str()
        .ok_or_else(|| unknown_event("SmartSoft response has no native event code"))?;
    let board = board(response);
    let active = match board.get("FreeSpinsCount") {
        Some(v) => {
            v.as_u64()
                .ok_or_else(|| unknown_event("SmartSoft FreeSpinsCount type changed"))?
                > 0
        }
        None => false,
    };
    if name == "start.game" {
        if !active || board["FreeSpinFinished"] == true {
            if !offered(response, "Start") {
                return Err(unknown_event(
                    "SmartSoft terminal start.game does not offer Start",
                ));
            }
            return Ok(None);
        }
        if offered(response, "Start") {
            return Ok(Some("Start"));
        }
    } else {
        // Published option serialization uses the native offer; unsupported choices stop.
        for action in ["PlayBonus", "FreeSpin", "PlayFreeSpin", "Finish"] {
            if offered(response, action) {
                return Ok(Some(action));
            }
        }
    }
    Err(unknown_event(format!(
        "SmartSoft unknown continuation event {name}"
    )))
}
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    ensure!(supports(game, mode), "unsupported SmartSoft mode");
    let (mut option, value) = selection(mode)?;
    let demo = open(ctx, &game.id).await?;
    let update = demo.endpoint("CustomEvent", None)?;
    ctx.json(
        "smartsoft_update_balance",
        update.as_str(),
        &json!({"EventCode":"update.balance"}),
        demo.headers(),
        false,
    )
    .await?;
    let events = demo.endpoint("Events", Some("0"))?;
    let mut current = decode(
        ctx.http(
            "smartsoft_initial",
            "GET",
            events.as_str(),
            demo.headers(),
            String::new(),
            false,
        )
        .await?,
    )?;
    ensure!(
        current["PlayerEvent"]["EventType"]["Code"] == "start.game" && offered(&current, "Start"),
        "SmartSoft initial start.game unavailable"
    );
    let pieces = selector(&current, demo.fraction_digits)?;
    if option == "PurchaseBonus" {
        ensure!(
            value.is_some_and(|v| board(&current)["BonusPrices"]
                .as_array()
                .is_some_and(|a| v < a.len() as u64)),
            "SmartSoft purchase index not offered"
        );
    }
    if option == "RiskBet"
        && let Some(a) = board(&current)["RiskBetMultipliers"]
            .as_array()
            .filter(|a| !a.is_empty())
    {
        ensure!(
            value.is_some_and(|v| v < a.len() as u64),
            "SmartSoft RiskBet index not offered"
        );
    }
    let endpoint = demo.endpoint("Event/2", None)?;
    for step in 0..ctx.max_steps() {
        ensure!(
            offered(&current, option),
            "SmartSoft requested native option is unavailable"
        );
        let event_id = &current["PlayerEvent"]["EventId"];
        if !event_id.is_string() && !event_id.is_number() {
            return Err(unknown_event("SmartSoft EventId missing"));
        }
        let selected_pieces = if step == 0 || matches!(option, "Start" | "PlayBonus") {
            &pieces
        } else {
            ""
        };
        let request = json!({"EventId":event_id,"SelectedOption":option,"SelectedPieces":selected_pieces,"SelectedValue":if step==0{value.map(|v|v.to_string())}else{None}});
        current = ctx
            .json(
                "smartsoft_event",
                endpoint.as_str(),
                &request,
                demo.headers(),
                true,
            )
            .await?;
        match continuation(&current)? {
            None => {
                let id = &current["PlayerEvent"]["EventId"];
                return Ok(Completion::new(if id.is_string() {
                    id.as_str().map(str::to_owned)
                } else if id.is_number() {
                    Some(id.to_string())
                } else {
                    None
                }));
            }
            Some(next) => option = next,
        }
    }
    Err(unknown_event("SmartSoft continuation limit reached"))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn response(code: &str, options: &[&str], free: u64) -> Value {
        json!({"PlayerEvent":{"EventId":17,"EventType":{"Code":code},"Options":options.iter().map(|o|json!({"OptionKey":o})).collect::<Vec<_>>()},"Board":{"MachineInfo":[{"FreeSpinsCount":free}]}})
    }
    #[test]
    fn native_terminal_needs_no_balance_currency_cost_or_payout() {
        assert_eq!(
            continuation(&response("start.game", &["Start"], 0)).unwrap(),
            None
        );
    }
    #[test]
    fn bonus_steps_continue_start_until_native_end() {
        assert_eq!(
            continuation(&response("start.game", &["Start"], 4)).unwrap(),
            Some("Start")
        );
        assert_eq!(
            continuation(&response("bonus", &["PlayBonus"], 4)).unwrap(),
            Some("PlayBonus")
        );
    }
    #[test]
    fn unknown_options_stop_with_typed_novelty() {
        let e = continuation(&response("new.phase", &["InventAction"], 0)).unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn selector_uses_only_required_native_fields() {
        let r =
            json!({"Board":{"MachineInfo":[{"LinePlay":20,"BetPerLine":"0.01","ChipCoef":"0.1"}]}});
        assert_eq!(selector(&r, 2).unwrap(), "20,0.01,10");
    }
    #[test]
    fn hidden_fields_support_entities_and_quoted_attributes() {
        let f = document_fields("<input id='Token' value='a&amp;b'><input value='abc' id=GameId>");
        assert_eq!(f["Token"], "a&b");
        assert_eq!(f["GameId"], "abc");
    }
}
