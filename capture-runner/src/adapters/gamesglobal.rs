//! Native capture protocol derived from standalone info/gamesglobal/codigo and the evidence reference adapter.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use roxmltree::{Document, Node};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::Url;
use uuid::Uuid;

const API: &str = "https://api32.gameassists.co.uk";
const PRODUCT: u32 = 22619;
const CLIENT: u32 = 50300;

fn module(game: &Game) -> Option<u32> {
    match game.id.as_str() {
        "GoldBlitzExtreme" => Some(101053),
        "12MasksofFireDrums" => Some(101417),
        _ => None,
    }
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    module(game).is_some() && mode.kind == ModeKind::Normal && mode.key == "normal"
}

fn query_is(url: &Url, key: &str, wanted: &str) -> bool {
    let mut values = url.query_pairs().filter(|(name, _)| name == key);
    values.next().is_some_and(|(_, v)| v == wanted) && values.next().is_none()
}

async fn get(ctx: &RunContext, label: &str, mut url: Url) -> Result<(Url, String)> {
    for _ in 0..7 {
        let reply = ctx.get(label, url.as_str()).await?;
        if reply.status == 200 {
            return Ok((url, reply.body));
        }
        ensure!(
            matches!(reply.status, 301 | 302 | 303 | 307 | 308),
            "Games Global launch HTTP {}",
            reply.status
        );
        url = url.join(
            reply
                .headers
                .get("location")
                .context("Games Global redirect target missing")?,
        )?;
    }
    bail!("Games Global launch redirect limit")
}

fn form(html: &str, base: &Url) -> Result<(Url, String)> {
    let forms = Regex::new(r"(?is)<form\b([^>]*)>(.*?)</form\s*>")?;
    let inputs = Regex::new(r"(?is)<input\b([^>]*)>")?;
    let attrs =
        Regex::new(r#"(?is)([a-z][a-z0-9_:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#)?;
    let attributes = |s: &str| {
        attrs
            .captures_iter(s)
            .map(|a| {
                let v = a
                    .get(2)
                    .or_else(|| a.get(3))
                    .or_else(|| a.get(4))
                    .unwrap()
                    .as_str();
                (
                    a[1].to_ascii_lowercase(),
                    v.replace("&quot;", "\"")
                        .replace("&#39;", "'")
                        .replace("&amp;", "&"),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let mut candidates = Vec::new();
    for parts in forms.captures_iter(html) {
        let attr = attributes(&parts[1]);
        let Some(action) = attr.get("action") else {
            continue;
        };
        let mut target = base.join(action)?;
        if target.path() != "/launcher/Generic" {
            continue;
        }
        let mut fields = BTreeMap::new();
        for input in inputs.captures_iter(&parts[2]) {
            let attr = attributes(&input[1]);
            if let Some(name) = attr.get("name").filter(|name| !name.is_empty()) {
                ensure!(
                    fields
                        .insert(name.clone(), attr.get("value").cloned().unwrap_or_default())
                        .is_none(),
                    "Games Global duplicate launch field"
                );
            }
        }
        ensure!(
            fields.get("playmode").is_some_and(|v| v == "demo"),
            "Games Global launch form is not demo"
        );
        let name = fields
            .get("gameName")
            .filter(|v| !v.is_empty())
            .context("Games Global launch game missing")?
            .clone();
        target.query_pairs_mut().extend_pairs(fields.iter());
        candidates.push((target, name));
    }
    ensure!(
        candidates.len() == 1,
        "Games Global launch form missing or ambiguous"
    );
    Ok(candidates.pop().unwrap())
}

async fn bootstrap(ctx: &RunContext, game: &Game, module: u32) -> Result<String> {
    let url = Url::parse(&format!(
        "https://www.gamesglobal.com/api/launch/{}?launchAsMobile=false",
        game.id
    ))?;
    let (url, html) = get(ctx, "demo_launch", url).await?;
    ensure!(
        matches!(
            url.host_str(),
            Some("redirector32.valueactive.eu" | "redirector32.valueactive.com")
        ) && query_is(&url, "playmode", "demo"),
        "Games Global demo redirect not confirmed"
    );
    let (target, name) = form(&html, &url)?;
    ensure!(
        name.eq_ignore_ascii_case(&format!("{}Desktop", game.id)),
        "Games Global launch game differs from inventory"
    );
    let (url, _) = get(ctx, "demo_form", target).await?;
    ensure!(
        matches!(
            url.host_str(),
            Some("mobile32.gameassists.co.uk" | "mobile32.gameassists.com")
        ) && query_is(&url, "playmode", "demo")
            && query_is(&url, "ispracticeplay", "true")
            && query_is(&url, "moduleid", &module.to_string())
            && query_is(&url, "clientid", &CLIENT.to_string())
            && query_is(&url, "productid", &PRODUCT.to_string())
            && query_is(&url, "casinoid", &PRODUCT.to_string())
            && query_is(&url, "gamename", &name),
        "Games Global practice identity changed"
    );
    Ok(url.origin().ascii_serialization())
}

async fn post(
    ctx: &RunContext,
    label: &str,
    path: &str,
    payload: &Value,
    headers: &BTreeMap<String, String>,
    financial: bool,
) -> Result<Value> {
    let mut headers = headers.clone();
    headers.insert("x-correlationid".into(), Uuid::new_v4().to_string());
    ctx.json(label, &format!("{API}{path}"), payload, headers, financial)
        .await
}

fn packet<'a>(value: &'a Value, module: u32, verb: &str) -> Result<Document<'a>> {
    let packet = &value["packet"];
    ensure!(
        packet["packetType"] == 8 && packet["isBase64Encoded"] == false,
        "Games Global packet encoding changed"
    );
    let xml = packet["payload"]
        .as_str()
        .filter(|v| v.len() <= 8 * 1024 * 1024)
        .context("Games Global packet missing")?;
    let doc = Document::parse(xml)?;
    let root = doc.root_element();
    let id = child(root, "Id")?;
    ensure!(
        root.has_tag_name("Pkt")
            && matches!(root.attribute("version"), None | Some("6.1"))
            && id.attribute("verb") == Some(verb)
            && id.attribute("mid") == Some(module.to_string().as_str())
            && id.attribute("cid") == Some(CLIENT.to_string().as_str())
            && id.attribute("sid") == Some(PRODUCT.to_string().as_str()),
        "Games Global native packet identity changed"
    );
    Ok(doc)
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Result<Node<'a, 'input>> {
    let mut children = node.children().filter(|v| v.has_tag_name(name));
    let value = children
        .next()
        .with_context(|| format!("Games Global {name} missing"))?;
    ensure!(children.next().is_none(), "Games Global {name} ambiguous");
    Ok(value)
}

fn attribute(node: Node<'_, '_>, name: &str) -> Result<u64> {
    node.attribute(name)
        .with_context(|| format!("Games Global {name} missing"))?
        .parse()
        .with_context(|| format!("Games Global {name} invalid"))
}

fn base<'a, 'input>(response: Node<'a, 'input>) -> Result<Node<'a, 'input>> {
    let core = child(response, "Core")?;
    let mut stages = core
        .children()
        .filter(|n| is_stage(*n))
        .filter(|n| n.attribute("avail") == Some("True"));
    let stage = stages.next().context("Games Global active stage missing")?;
    if !(stages.next().is_none()
        && stage.attribute("name") == Some("BaseGame")
        && stage.attribute("index").is_some()
        && stage.attribute("index") == core.attribute("next")
        && attribute(stage, "remain")? == 1)
    {
        return Err(unknown_event("Games Global round has an unresolved stage"));
    }
    for other in core.children().filter(|n| is_stage(*n) && *n != stage) {
        if !(attribute(other, "remain")? == 0) {
            return Err(unknown_event("Games Global round has a pending stage"));
        }
    }
    Ok(stage)
}

fn is_stage(node: Node<'_, '_>) -> bool {
    node.has_tag_name("Stage") || node.has_tag_name("Feature")
}

fn wager(response: Node<'_, '_>, stage: Node<'_, '_>) -> Result<(u64, u64, u64)> {
    let normal = if stage.children().any(|n| n.has_tag_name("PossibleBets")) {
        let mut actions = child(stage, "PossibleBets")?
            .children()
            .filter(|n| n.has_tag_name("Action") && n.attribute("value") == Some("Play"));
        let action = actions.next().context("Games Global Play not offered")?;
        ensure!(
            actions.next().is_none(),
            "Games Global Play offer ambiguous"
        );
        child(action, "Bet")?
    } else {
        child(stage, "Bet")?
    };
    ensure!(
        matches!(normal.attribute("name"), Some("NormalBet" | "BaseBet"))
            && normal.attributes().len() == 2,
        "Games Global normal wager ambiguous"
    );
    let settings = child(response, "Settings")?;
    let level = attribute(settings, "minlevel")?;
    let denom = settings
        .attribute("denoms")
        .context("Games Global denomination menu missing")?
        .split(',')
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .min()
        .context("Games Global denomination menu empty")?;
    let cost = attribute(normal, "principal")?
        .checked_mul(level)
        .and_then(|v| v.checked_mul(denom))
        .filter(|v| *v > 0)
        .context("Games Global invalid minimum wager")?;
    Ok((cost, level, denom))
}

fn request(module: u32, attributes: &str) -> Value {
    json!({"packet":{"packetType":7,"payload":format!("<Pkt version=\"6.1\"><Id mid=\"{module}\" cid=\"{CLIENT}\" sid=\"{PRODUCT}\" verb=\"AdvSlot\" /><Request {attributes} /></Pkt>"),
         "useFilter":true,"filter":"AdvancedSlotsFilter","isBase64Encoded":false},"context":{}})
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &serde_json::Value,
) -> Result<Completion> {
    ensure!(supports(game, mode), "Games Global unsupported slot mode");
    let module = module(game).context("Games Global module missing")?;
    let origin = bootstrap(ctx, game, module).await?;
    let mut headers = BTreeMap::from([
        ("origin".into(), origin.clone()),
        ("referer".into(), format!("{origin}/")),
        ("x-clienttypeid".into(), "70".into()),
    ]);
    let attributes = [
        ("bn", "Mozilla"),
        ("dos", "Windows"),
        ("dosv", "10"),
        ("it", "false"),
        ("mktn", ""),
        ("mbb", "Firefox"),
        ("mbbv", "159.0"),
        ("mdln", "Firefox"),
        (
            "ua",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:159.0) Gecko/20100101 Firefox/159.0",
        ),
        ("ff", "desktop"),
    ];
    let device = attributes
        .into_iter()
        .map(|(key, value)| json!({"shortCode":key,"value":value}))
        .collect::<Vec<_>>();
    let login=post(ctx,"demo_login","/casino/user/public/v1/accounts/login/demo?fields=core,balance,session",&json!({
        "sessionProductId":PRODUCT,"numLaunchTokens":10,"environment":{"clientTypeId":70,"languageCode":"en"},
        "deviceAttributes":device,"context":{"moduleId":module,"clientId":CLIENT,"intent":1}
    }),&headers,false).await?;
    let core = &login["account"]["core"];
    ensure!(
        core["userTypeId"] == 5
            && core["isExternalAccount"] == false
            && core["isExternalBalance"] == false,
        "Games Global login is not anonymous demo"
    );
    let launch_token = login["tokens"]["launchTokens"][0]
        .as_str()
        .filter(|v| !v.is_empty())
        .context("Games Global launch token missing")?;
    let mut auth_headers = headers.clone();
    auth_headers.insert("hostapplicationid".into(), "2".into());
    auth_headers.insert("cwasessionid".into(), Uuid::new_v4().to_string());
    let auth = post(
        ctx,
        "authenticate",
        "/Shared/Communications/Public/v1/accounts/authenticate",
        &json!({
            "launchToken":launch_token,"numberOfLaunchTokens":5,"productId":PRODUCT
        }),
        &auth_headers,
        false,
    )
    .await?;
    ensure!(
        auth["userType"] == 5 && auth["sessionProductId"] == PRODUCT,
        "Games Global authentication is not demo"
    );
    let token = auth["userTokenDetails"]["userToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("Games Global user token missing")?;
    headers.insert("authorization".into(), format!("Bearer {token}"));
    headers.insert("x-route-productid".into(), PRODUCT.to_string());
    headers.insert("x-route-moduleid".into(), module.to_string());
    let path = format!("/casino/play/public/v1/games/module/{module}/client/{CLIENT}/play");
    let initial = post(
        ctx,
        "refresh",
        &path,
        &request(module, r#"verbex="Refresh""#),
        &headers,
        false,
    )
    .await?;
    ensure!(
        initial["context"]["isReadOnly"] == true,
        "Games Global Refresh is not read-only"
    );
    let initial_xml = packet(&initial, module, "Refresh")?;
    let old = child(initial_xml.root_element(), "Response")?;

    let (cost, level, denom) = wager(old, base(old)?)?;

    ctx.save_session(
        &json!({"game":game.id,"module":module,"client":CLIENT,"product":PRODUCT,"token":token}),
    )?;
    let entry = request(
        module,
        &format!(
            r#"verbex="Spin" action="Play" name="BaseGame" denom="{denom}" level="{level}" wager="{cost}""#
        ),
    );
    let reply = post(ctx, "normal", &path, &entry, &headers, true).await?;
    ensure!(
        reply["context"]["isReadOnly"] == false,
        "Games Global spin not accepted"
    );
    let xml = packet(&reply, module, "Spin")?;
    let settled = child(xml.root_element(), "Response")?;
    base(settled)?;
    let transaction = settled
        .attribute("tnum")
        .filter(|v| !v.is_empty() && Some(*v) != old.attribute("tnum"))
        .context("Games Global transaction identity did not advance")?;
    Ok(Completion {
        native_round_id: Some(transaction.into()),
        details: json!({"module":module,"client":CLIENT,"product":PRODUCT}),
    })
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn terminal_base_stage_requires_no_user_balance() {
        let doc=Document::parse(r#"<Response><Core next="0"><Stage avail="True" name="BaseGame" index="0" remain="1"/><Feature remain="0"/></Core></Response>"#).unwrap();
        assert_eq!(
            base(doc.root_element()).unwrap().attribute("name"),
            Some("BaseGame")
        );
    }
    #[test]
    fn new_stage_is_typed_novelty() {
        let doc=Document::parse(r#"<Response><Core next="1"><Feature avail="True" name="NewFeature" index="1" remain="2"/></Core></Response>"#).unwrap();
        assert!(
            base(doc.root_element())
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
