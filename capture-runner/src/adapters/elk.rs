//! Native ELK stage demos: ECF 1/2/3, modern/classic XML, variants and Boss progression.
//! Source: standalone info/elk/referencias/demo-dashboard/elk_{client,protocol,boss}.py.
//! Exact source hashes and all 159 catalogue bindings are in bindings/elk-bindings.json.
//! Amounts below encode published requests; wallets and awards never gate capture.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, decimal, now_ms, unknown_event},
};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use roxmltree::{Document, Node};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
    time::Duration,
};
use tokio::time::Instant;
use url::Url;
use uuid::Uuid;

const STATIC: &str = "https://static-stage.contentmedia.eu";
const CAPI: &str = "https://gc5-stage.contentmedia.eu/capi";
const WORLDS: [&str; 3] = ["temple", "ship", "nest"];

#[derive(Deserialize)]
struct Binding {
    id: String,
    xdm: String,
    direct_name: Option<String>,
    modes: BTreeMap<String, String>,
}
fn bindings() -> &'static BTreeMap<String, Binding> {
    #[derive(Deserialize)]
    struct Published {
        games: BTreeMap<String, Binding>,
    }
    static DATA: OnceLock<BTreeMap<String, Binding>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str::<Published>(include_str!("../../bindings/elk-bindings.json"))
            .expect("embedded ELK bindings")
            .games
    })
}
pub fn supports(game: &Game, mode: &Mode) -> bool {
    bindings().get(&game.id).is_some_and(|b| {
        b.modes
            .get(&mode.key)
            .is_some_and(|kind| kind == mode.kind.as_str())
    })
}
pub fn validate(game: &Game, mode: &Mode, settings: &Value) -> Result<()> {
    ensure!(
        supports(game, mode),
        "ELK selector is absent from published bindings"
    );
    let binding = &bindings()[&game.id];
    for (key, expected) in [("game_id", &binding.id), ("xdm", &binding.xdm)] {
        if let Some(value) = settings.get(key) {
            ensure!(
                value.as_str() == Some(expected),
                "ELK configured {key} differs from public binding"
            );
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
struct Offer {
    level: String,
    amount: Decimal,
    mode: String,
    variant: Option<String>,
    uses_bmode: bool,
}
impl Offer {
    fn key(&self) -> String {
        self.variant
            .as_ref()
            .map_or_else(|| self.mode.clone(), |v| format!("{}:{v}", self.mode))
    }
}
fn xml(text: &str) -> Result<Document<'_>> {
    let upper = text.to_ascii_uppercase();
    if upper.contains("<!DOCTYPE") || upper.contains("<!ENTITY") {
        return Err(unknown_event("ELK unknown XML declaration"));
    }
    Document::parse(text).context("invalid native ELK XML")
}
fn node<'a, 'input>(mut root: Node<'a, 'input>, path: &str) -> Result<Node<'a, 'input>> {
    for part in path.split('/') {
        let mut matches = root.children().filter(|n| n.has_tag_name(part));
        root = matches
            .next()
            .ok_or_else(|| unknown_event(format!("ELK missing XML field {path}")))?;
        if matches.next().is_some() {
            return Err(unknown_event(format!("ELK duplicate XML field {path}")));
        }
    }
    Ok(root)
}
fn value(root: Node<'_, '_>, path: &str) -> Result<String> {
    node(root, path)?
        .text()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| unknown_event(format!("ELK empty XML field {path}")))
}
fn response<'a>(text: &'a str, method: &str) -> Result<Document<'a>> {
    let document = xml(text)?;
    let root = document.root_element();
    if root.tag_name().name() != "response" || value(root, "method")? != method {
        return Err(unknown_event("ELK native method or response shape changed"));
    }
    let error = value(root, "errorcode")?;
    ensure!(error == "0", "ELK provider rejection: {error}");
    Ok(document)
}
fn number(raw: &str) -> Result<Decimal> {
    decimal(raw).map_err(|_| unknown_event("ELK malformed native numeric field"))
}
fn identifier(raw: &str, mode: bool) -> bool {
    !raw.is_empty()
        && if mode {
            raw.as_bytes()[0].is_ascii_uppercase()
                && raw
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        } else {
            raw.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }
}

/// Discover fresh native minimum selectors, including per-variant menus.
fn initial_offers(text: &str) -> Result<BTreeMap<String, Offer>> {
    let document = response(text, "init")?;
    let root = document.root_element();
    ensure!(
        value(root, "roundid")? == "0",
        "ELK demo is not a fresh native session"
    );
    value(root, "gamesession/id")?;
    let levels = node(root, "betlevels")?;
    let uses_bmode = match levels.attribute("bmode") {
        Some("true") => true,
        Some("false") => false,
        _ => return Err(unknown_event("ELK unrecognized bet-mode flag")),
    };
    let enabled = uses_bmode
        && root
            .children()
            .find(|n| n.has_tag_name("gameprops"))
            .and_then(|n| n.children().find(|n| n.has_tag_name("bmode")))
            .is_none_or(|n| n.attribute("enabled") == Some("true"));
    let mut found: BTreeMap<String, (Decimal, u64, Offer)> = BTreeMap::new();
    for level in levels.children().filter(|n| n.has_tag_name("betlevel")) {
        let id = level.attribute("id").unwrap_or("");
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(unknown_event("ELK unrecognized bet-level identifier"));
        }
        let numeric_id = id
            .parse::<u64>()
            .map_err(|_| unknown_event("ELK bet-level overflow"))?;
        let modes: Vec<_> = level
            .children()
            .filter(|n| n.has_tag_name("bmodes"))
            .flat_map(|n| n.children().filter(|n| n.has_tag_name("bmode")))
            .map(|n| {
                (
                    n.attribute("id").unwrap_or(""),
                    n.attribute("amount").unwrap_or(""),
                )
            })
            .collect();
        let modes = if modes.is_empty() {
            vec![("DEF", level.attribute("amount").unwrap_or(""))]
        } else {
            modes
        };
        let mut seen = BTreeSet::new();
        for (key, raw) in modes {
            if key != "DEF" && !enabled {
                continue;
            }
            if !identifier(key, true) || !seen.insert(key) {
                return Err(unknown_event("ELK unknown or duplicate native mode"));
            }
            let amount = number(raw)?;
            if amount <= Decimal::ZERO {
                continue;
            }
            let candidate = Offer {
                level: id.into(),
                amount,
                mode: key.into(),
                variant: None,
                uses_bmode,
            };
            if found
                .get(key)
                .is_none_or(|(old, index, _)| (amount, numeric_id) < (*old, *index))
            {
                found.insert(key.into(), (amount, numeric_id, candidate));
            }
        }
    }
    ensure!(
        found.contains_key("DEF"),
        "ELK normal selector is not offered"
    );
    let variants: Vec<_> = root
        .children()
        .filter(|n| n.has_tag_name("variants"))
        .flat_map(|n| n.children().filter(|n| n.has_tag_name("variant")))
        .collect();
    if variants.is_empty() {
        return Ok(found
            .into_iter()
            .map(|(key, (_, _, offer))| (key, offer))
            .collect());
    }
    let mut expanded = BTreeMap::new();
    for variant in variants {
        let name = variant.attribute("id").unwrap_or("");
        if !identifier(name, false) {
            return Err(unknown_event("ELK unknown variant identifier"));
        }
        let table_text = value(variant, "game/paytable")?;
        let table = xml(&table_text)?;
        let offered: BTreeSet<_> = table
            .root_element()
            .children()
            .filter(|n| n.has_tag_name("BMODES"))
            .flat_map(|n| n.children().filter(|n| n.has_tag_name("BMODE")))
            .filter_map(|n| n.attribute("name"))
            .collect();
        for (_, _, offer) in found.values() {
            if !offered.is_empty() && !offered.contains(offer.mode.as_str()) {
                continue;
            }
            let mut offer = offer.clone();
            offer.variant = Some(name.into());
            if expanded.insert(offer.key(), offer).is_some() {
                return Err(unknown_event("ELK duplicate variant selector"));
            }
        }
    }
    ensure!(!expanded.is_empty(), "ELK variant menu is empty");
    Ok(expanded)
}
fn selected_paytable(init: Node<'_, '_>, offer: &Offer) -> Result<String> {
    if let Some(name) = &offer.variant {
        let variant = node(init, "variants")?
            .children()
            .find(|n| n.has_tag_name("variant") && n.attribute("id") == Some(name.as_str()))
            .ok_or_else(|| unknown_event("ELK selected variant disappeared"))?;
        value(variant, "game/paytable")
    } else {
        value(init, "game/paytable")
    }
}
fn result_closure(init: Node<'_, '_>, purchase: Node<'_, '_>, offer: &Offer) -> Result<usize> {
    let table_text = selected_paytable(init, offer)?;
    let table_doc = xml(&table_text)?;
    let table = table_doc.root_element();
    let mut modes: BTreeSet<Option<String>> = table
        .children()
        .filter(|n| n.has_tag_name("MODE"))
        .filter_map(|n| n.attribute("name").map(|v| Some(v.into())))
        .collect();
    let results: Vec<_> = purchase.children().filter(|n| n.is_element()).collect();
    if results.is_empty() || results.iter().any(|n| !n.has_tag_name("RESULT")) {
        return Err(unknown_event("ELK unrecognized purchase continuation"));
    }
    if modes.is_empty() && !offer.uses_bmode {
        let configs: Vec<_> = table
            .children()
            .filter(|n| n.has_tag_name("CONFIG"))
            .collect();
        if configs.is_empty() {
            return Err(unknown_event("ELK unknown classic layout"));
        }
        if configs.iter().all(|n| n.attribute("id").is_some()) {
            modes.extend(
                configs
                    .iter()
                    .map(|n| Some(n.attribute("id").unwrap().into())),
            );
        } else if configs.iter().all(|n| n.attribute("id").is_none()) {
            modes.extend((0..configs.len()).map(|i| Some(i.to_string())));
        } else {
            return Err(unknown_event("ELK mixed classic identifiers"));
        }
        if table.has_tag_name("MODES") {
            for result in &results {
                if result.attribute("mode").is_none()
                    && !matches!(
                        result
                            .attribute("freespin")
                            .or_else(|| result.attribute("freespins")),
                        Some("0" | "1")
                    )
                {
                    return Err(unknown_event("ELK unknown classic spin flag"));
                }
            }
            modes.insert(None);
        }
    }
    if table.attribute("gameid") == Some("10053")
        && modes.contains(&Some("FREESPINS-AVALANCHE".into()))
    {
        let aliases: Vec<_> = modes
            .iter()
            .flatten()
            .filter(|s| {
                matches!(
                    s.as_str(),
                    "AVALANCHE-4" | "AVALANCHE-5" | "AVALANCHE-6" | "AVALANCHE-7" | "AVALANCHE-8"
                )
            })
            .map(|s| Some(format!("FREESPINS-{s}")))
            .collect();
        modes.extend(aliases);
    }
    for (index, result) in results.iter().enumerate() {
        if !modes.contains(&result.attribute("mode").map(str::to_owned)) {
            return Err(unknown_event("ELK unpublished result mode"));
        }
        if results.iter().any(|n| n.attribute("resultIndex").is_some())
            && result
                .attribute("resultIndex")
                .and_then(|s| s.parse::<usize>().ok())
                != Some(index)
        {
            return Err(unknown_event("ELK changed native result order"));
        }
    }
    let last = *results.last().unwrap();
    for key in ["freespin", "freespins", "fsleft"] {
        if let Some(raw) = last.attribute(key)
            && number(raw)? != Decimal::ZERO
        {
            return Err(unknown_event("ELK classic bonus continuation remains"));
        }
    }
    for action in last.children().filter(|n| n.has_tag_name("ACTIONS")) {
        if let Some(raw) = action.attribute("fscount")
            && number(raw)? != Decimal::ZERO
        {
            return Err(unknown_event("ELK classic bonus actions remain"));
        }
    }
    let known = [
        "bonus-counter",
        "freedrops",
        "redrops",
        "free-drops",
        "free-spins",
        "respins",
        "freespins",
        "subrounds",
        "dropz",
        "respins-base",
        "respins-bonus",
        "extra-drops",
        "kitty-free-drops",
        "drops",
        "coin-game-counter",
        "lives",
        "sub-rounds",
        "coin-chase-counter",
        "",
        "bonus-symbols",
        "super-bonus-symbols",
        "both-ways",
        "life-lines",
        "bonus spins",
        "casino spins",
        "spins",
        "extras",
        "bonus-spins",
        "progress",
        "base-multiplier",
        "levels",
        "features-collected",
        "bonus-respins",
    ];
    let meters = [
        "win-multiplier",
        "multiplier",
        "collect-bonuses",
        "body-counter",
        "win multiplier",
        "remembered-multiplier",
    ];
    for counter in last
        .children()
        .filter(|n| n.has_tag_name("COUNTERS"))
        .flat_map(|n| n.children().filter(|n| n.has_tag_name("COUNTER")))
    {
        let name = counter
            .attribute("name")
            .ok_or_else(|| unknown_event("ELK counter name missing"))?;
        if !known.contains(&name) && !meters.contains(&name) {
            return Err(unknown_event(format!(
                "ELK unknown continuation counter {name}"
            )));
        }
        if !meters.contains(&name)
            && number(counter.attribute("left").unwrap_or("0"))? > Decimal::ZERO
        {
            let fish_summary = table.attribute("gameid") == Some("10246")
                && name == "bonus-counter"
                && counter.attribute("current").is_none()
                && counter.attribute("decrease").is_none()
                && last.descendants().any(|n| {
                    n.has_tag_name("ACTION")
                        && n.attribute("name") == Some("bonus-summary")
                        && n.attribute("bonus-counter-left") == Some("0")
                });
            if !fish_summary {
                return Err(unknown_event("ELK native continuation counter remains"));
            }
        }
    }
    Ok(results.len())
}

fn settlement(initial: &str, purchased: &str, checked: &str, offer: &Offer) -> Result<Completion> {
    let init_doc = response(initial, "init")?;
    let purchase_doc = response(purchased, "purchase")?;
    let check_doc = response(checked, "checkwin")?;
    let round = value(purchase_doc.root_element(), "roundid")?;
    if round == "0" || value(check_doc.root_element(), "roundid")? != round {
        return Err(unknown_event("ELK round identity changed"));
    }
    if let Some(info) = purchase_doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("roundinfo"))
    {
        for (key, expected) in [
            ("roundid", round.as_str()),
            ("bmode", offer.mode.as_str()),
            ("betlevel", offer.level.as_str()),
        ] {
            if info.children().any(|n| n.has_tag_name(key)) && value(info, key)? != expected {
                return Err(unknown_event(format!(
                    "ELK native roundinfo {key} differs from requested round"
                )));
            }
        }
    }
    let data = value(purchase_doc.root_element(), "game/pubdata")?;
    let pubdoc = xml(&data)?;
    let root = pubdoc.root_element();
    if !root.has_tag_name("PURCHASES") || root.children().filter(|n| n.is_element()).count() != 1 {
        return Err(unknown_event("ELK unrecognized purchase sequence"));
    }
    let native = node(root, "PURCHASE")?;
    let expected_mode = if offer.uses_bmode {
        Some(offer.mode.as_str())
    } else {
        None
    };
    if native.attribute("bmode") != expected_mode
        || native.attribute("variant") != offer.variant.as_deref()
    {
        return Err(unknown_event("ELK response selector differs from request"));
    }
    if let Some(level) = native.attribute("betlevel") {
        if level != offer.level {
            return Err(unknown_event("ELK response bet level changed"));
        }
    } else {
        let text = selected_paytable(init_doc.root_element(), offer)?;
        let table = xml(&text)?;
        let table = table.root_element();
        if let Some(revision) = native
            .attribute("revision")
            .or_else(|| table.attribute("revision"))
        {
            if native.attribute("revision") != Some(revision)
                || table.attribute("revision") != Some(revision)
            {
                return Err(unknown_event("ELK classic protocol revision changed"));
            }
        } else if offer.uses_bmode
            || !(table.attribute("protocol-version") == Some("2")
                || (matches!(table.tag_name().name(), "SETUP" | "MODES")
                    && table.children().any(|n| n.has_tag_name("CONFIG"))))
        {
            return Err(unknown_event("ELK unrecognized legacy purchase layout"));
        }
    }
    let steps = result_closure(init_doc.root_element(), native, offer)?;
    Ok(Completion {
        native_round_id: Some(round),
        details: json!({"native_results":steps,"mode":offer.mode,"variant":offer.variant}),
    })
}

fn launch_binding(location: &str, game_id: &str) -> Result<(String, String)> {
    let parsed = Url::parse(location)?;
    ensure!(
        parsed.scheme() == "https"
            && parsed.host_str() == Some("static-stage.contentmedia.eu")
            && parsed.port().is_none()
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.fragment().is_none(),
        "unverified public ELK launcher authority"
    );
    if !matches!(
        parsed.path(),
        "/ecf3/index.html" | "/ecf2/index.html" | "/ecf/index.html"
    ) {
        return Err(unknown_event("ELK unrecognized framework path"));
    }
    let mut query = BTreeMap::new();
    for (k, v) in parsed.query_pairs() {
        if query.insert(k.into_owned(), v.into_owned()).is_some() {
            return Err(unknown_event("ELK duplicate launcher parameter"));
        }
    }
    for (key, expected) in [
        ("gameid", game_id),
        ("operatorid", "44"),
        ("mode", "demo"),
        ("currency", "EUR"),
        ("capi", CAPI),
    ] {
        ensure!(
            query.get(key).map(String::as_str) == Some(expected),
            "ELK official demo launcher contract mismatch: {key}"
        );
    }
    let name = query
        .get("gamename")
        .filter(|s| identifier(s, false) && s.bytes().all(|b| !b.is_ascii_uppercase()))
        .ok_or_else(|| unknown_event("ELK unrecognized public game name"))?;
    Ok((
        name.clone(),
        parsed.path().split('/').nth(1).unwrap().into(),
    ))
}
fn request_xml(attributes: &BTreeMap<String, String>) -> String {
    let mut text = String::from("<?xml version=\"1.0\"?><request");
    for (name, raw) in attributes {
        let escaped = raw
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;");
        text.push_str(&format!(" {name}=\"{escaped}\""));
    }
    text.push_str(" />");
    text
}
struct Session<'a> {
    ctx: &'a RunContext,
    game_id: &'a str,
    headers: BTreeMap<String, String>,
    key: String,
    last_rid: u64,
    spin_delay: Duration,
    last_purchase: Option<Instant>,
}
impl Session<'_> {
    async fn call(
        &mut self,
        method: &str,
        attributes: BTreeMap<String, String>,
        round_action: bool,
    ) -> Result<String> {
        if !matches!(method, "init" | "purchase" | "checkwin") {
            return Err(unknown_event("ELK unknown native method"));
        }
        if method == "purchase"
            && let Some(last) = self.last_purchase
        {
            tokio::time::sleep_until(last + self.spin_delay).await;
        }
        let rid = now_ms()
            .checked_mul(10)
            .context("ELK request id overflow")?
            .max(
                self.last_rid
                    .checked_add(1)
                    .context("ELK request id overflow")?,
            );
        self.last_rid = rid;
        let mut headers = self.headers.clone();
        headers.insert("x-capi-rid".into(), format!("{rid}:1"));
        let reply = self
            .ctx
            .http(
                method,
                "POST",
                &format!(
                    "{CAPI}/{}/play/{method}?gl=true&standalone=false&iframe=false",
                    self.game_id
                ),
                headers,
                request_xml(&attributes),
                round_action,
            )
            .await?;
        if method == "purchase" {
            self.last_purchase = Some(Instant::now());
        }
        ensure!(
            reply.status == 200,
            "native ELK HTTP failure: {}",
            reply.status
        );
        let document = response(&reply.body, method)?;
        if let Some(session) = document
            .root_element()
            .children()
            .find(|n| n.has_tag_name("gamesession"))
            && session.children().any(|n| n.has_tag_name("id"))
        {
            self.key = value(session, "id")?;
        }
        Ok(reply.body)
    }
    async fn play(&mut self, offer: &Offer) -> Result<(String, String)> {
        let mut attrs = BTreeMap::from([
            ("gamesession".into(), self.key.clone()),
            ("amount".into(), offer.amount.to_string()),
            ("cur".into(), "EUR".into()),
            ("betdetails".into(), String::new()),
            ("betlevel".into(), offer.level.clone()),
            ("pmode".into(), "click-s0".into()),
            ("orientation".into(), "landscape".into()),
            ("sound".into(), "bgeffect".into()),
            ("monetaryunit".into(), "money".into()),
            ("gamespeed".into(), "normal".into()),
        ]);
        if offer.uses_bmode {
            attrs.insert("bmode".into(), offer.mode.clone());
        }
        if let Some(variant) = &offer.variant {
            attrs.insert("variant".into(), variant.clone());
        }
        let purchased = self.call("purchase", attrs, true).await?;
        let round = value(response(&purchased, "purchase")?.root_element(), "roundid")?;
        if round == "0" {
            return Err(unknown_event("ELK accepted round identifier missing"));
        }
        let checked = self
            .call(
                "checkwin",
                BTreeMap::from([
                    ("gamesession".into(), self.key.clone()),
                    ("roundid".into(), round),
                ]),
                true,
            )
            .await?;
        Ok((purchased, checked))
    }
}
fn progress(text: &str) -> Result<BTreeMap<String, u8>> {
    let document = xml(text)?;
    WORLDS
        .iter()
        .map(|world| {
            let name = format!("collected-tokens-{world}");
            let mut nodes = document
                .root_element()
                .children()
                .filter(|n| n.has_tag_name("VAR") && n.attribute("id") == Some(name.as_str()));
            let state = nodes
                .next()
                .ok_or_else(|| unknown_event(format!("ELK native progress missing for {world}")))?;
            if nodes.next().is_some() {
                return Err(unknown_event("ELK duplicate progress state"));
            }
            let mask: u8 = value(state, "STATE")?
                .parse()
                .map_err(|_| unknown_event("ELK invalid progress mask"))?;
            if mask > 7 {
                return Err(unknown_event("ELK unrecognized progress mask"));
            }
            Ok(((*world).into(), mask))
        })
        .collect()
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    validate(game, mode, settings)?;
    let binding = &bindings()[&game.id];
    let direct = binding.direct_name.is_some();
    let mut launcher = Url::parse(if direct {
        "https://static-stage.contentmedia.eu/ecf3/index.html"
    } else {
        "https://gamelauncher-stage.contentmedia.eu/1.0/game/"
    })?;
    launcher.query_pairs_mut().extend_pairs([
        ("gameid", binding.id.as_str()),
        ("operatorid", "44"),
        ("mode", "demo"),
        ("currency", "EUR"),
        ("device", "desktop"),
        ("language", "en_gb"),
        ("xdm", binding.xdm.as_str()),
    ]);
    let location = if let Some(name) = &binding.direct_name {
        launcher.query_pairs_mut().extend_pairs([
            ("gamename", name.as_str()),
            ("capi", CAPI),
            ("papi", "https://papi-stage.contentmedia.eu"),
        ]);
        launcher.to_string()
    } else {
        let launched = ctx.get("launcher", launcher.as_str()).await?;
        ensure!(
            matches!(launched.status, 301 | 302 | 303 | 307 | 308),
            "ELK public launcher did not return a redirect"
        );
        launched
            .headers
            .get("location")
            .ok_or_else(|| unknown_event("ELK official launcher location missing"))?
            .clone()
    };
    let (name, framework) = launch_binding(&location, &binding.id)?;
    let game_source = ctx
        .get("version", &format!("{STATIC}/{name}/version.js"))
        .await?;
    let framework_source = ctx
        .get("version", &format!("{STATIC}/{framework}/version.js"))
        .await?;
    ensure!(
        game_source.status == 200 && framework_source.status == 200,
        "ELK official version unavailable"
    );
    fn revision(source: &str, key: &str) -> Result<String> {
        let pattern = Regex::new(&format!(
            r#"\b{}\s*=\s*["']([0-9]+)["']"#,
            regex::escape(key)
        ))?;
        let mut matches = pattern.captures_iter(source);
        let first = matches
            .next()
            .ok_or_else(|| unknown_event("ELK public build revision missing"))?;
        if matches.next().is_some() {
            return Err(unknown_event("ELK ambiguous public revision"));
        }
        Ok(first[1].to_owned())
    }
    let client_revision = revision(&game_source.body, "game_revision")?;
    let ecf_revision = revision(&framework_source.body, "ecf_revision")?;
    let headers = BTreeMap::from([
        ("origin".into(), STATIC.into()),
        ("content-type".into(), "text/xml; charset=utf-8".into()),
        ("x-capi-version".into(), "2.0".into()),
        ("x-client-version".into(), client_revision.clone()),
        ("x-ecf-version".into(), ecf_revision.clone()),
    ]);
    let external = now_ms()
        .checked_add((Uuid::new_v4().as_u128() % 10000) as u64)
        .context("ELK external session overflow")?
        .to_string();
    let mut session = Session {
        ctx,
        game_id: &binding.id,
        headers,
        key: String::new(),
        last_rid: 0,
        spin_delay: Duration::ZERO,
        last_purchase: None,
    };
    let initial = session
        .call(
            "init",
            BTreeMap::from([
                ("opid".into(), "44".into()),
                ("extsession".into(), external.clone()),
                ("cur".into(), "EUR".into()),
                ("desktopmode".into(), "true".into()),
                ("lang".into(), "en_gb".into()),
                ("screenres".into(), "1920,1080,1718,966".into()),
                ("extjur".into(), "NA".into()),
            ]),
            false,
        )
        .await?;
    let offers = initial_offers(&initial)?;
    if let Some(new) = offers.keys().find(|key| !binding.modes.contains_key(*key)) {
        return Err(unknown_event(format!(
            "ELK unrecorded native selector {new}"
        )));
    }
    let offer = offers
        .get(&mode.key)
        .context("ELK requested mode is no longer offered")?;
    let initial_doc = response(&initial, "init")?;
    let session_node = node(initial_doc.root_element(), "gamesession")?;
    let delay = session_node
        .children()
        .find(|n| n.has_tag_name("minspindelay"))
        .map(|n| n.text().unwrap_or("0"))
        .unwrap_or("0")
        .parse::<u64>()
        .map_err(|_| unknown_event("ELK malformed native spin delay"))?;
    if delay > 30000 {
        return Err(unknown_event("ELK unsupported native spin delay"));
    }
    session.spin_delay = Duration::from_millis(if delay == 0 { 0 } else { delay + 100 });
    ctx.save_session(&json!({"provider":"elk","game":game.id,"game_id":binding.id,"initial_gamesession":session.key,"external_session":external,"client_revision":client_revision,"ecf_revision":ecf_revision,"currency":"EUR","mode":"demo","operator_id":"44","requested_mode":mode.key}))?;
    let mut prerequisite_rounds = 0;
    let mut seen = BTreeSet::new();
    if binding.id == "10266" && offer.variant.as_deref() == Some("boss") {
        let mut state = progress(&value(
            initial_doc.root_element(),
            "variants/pubstate/init",
        )?)?;
        let progress_mode = settings
            .get("boss_progress_mode")
            .and_then(Value::as_str)
            .unwrap_or("X04");
        while let Some(world) = WORLDS.iter().find(|world| state[**world] != 7) {
            ensure!(
                prerequisite_rounds < ctx.max_steps(),
                "ELK native Boss progression reached max_steps before all tokens were collected"
            );
            let selector = format!("{progress_mode}:{world}");
            let prerequisite = offers
                .get(&selector)
                .context("ELK published Boss progression mode unavailable")?;
            ctx.note("native_prerequisite", &json!({"target":mode.key,"selector":selector,"progress_before":state,"sequence":prerequisite_rounds + 1}))?;
            let (buy, check) = session.play(prerequisite).await?;
            let closed = settlement(&initial, &buy, &check, prerequisite)?;
            if !seen.insert(closed.native_round_id) {
                return Err(unknown_event("ELK duplicate progression round identifier"));
            }
            state = progress(&value(
                response(&buy, "purchase")?.root_element(),
                "game/pubstate/roundend",
            )?)?;
            prerequisite_rounds += 1;
        }
    }
    let (purchased, checked) = session.play(offer).await?;
    let mut completed = settlement(&initial, &purchased, &checked, offer)?;
    if !seen.insert(completed.native_round_id.clone()) {
        return Err(unknown_event("ELK duplicate requested round identifier"));
    }
    completed.details["game_id"] = json!(binding.id);
    completed.details["prerequisite_rounds"] = json!(prerequisite_rounds);
    Ok(completed)
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn every_archived_elk_selector_has_a_native_connector() {
        let provider: crate::model::Provider =
            serde_json::from_str(include_str!("../../../standalone info/elk/inventario.json"))
                .unwrap();
        assert_eq!(provider.games.len(), 159);
        let mut count = 0;
        for game in &provider.games {
            for mode in &game.modes {
                assert!(supports(game, mode), "{}/{}", game.id, mode.key);
                count += 1;
            }
        }
        assert_eq!(count, 803);
    }
    #[test]
    fn all_803_selectors_are_reconstructed_from_real_native_initializations() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../standalone info/elk/referencias/demo-dashboard/elk/batches");
        let mut count = 0;
        for batch in ["1-100", "101-159"] {
            let rows: Vec<Value> = serde_json::from_slice(
                &std::fs::read(root.join(batch).join("inventory.json")).unwrap(),
            )
            .unwrap();
            for row in rows {
                let offers = initial_offers(row["init"].as_str().unwrap())
                    .unwrap_or_else(|e| panic!("{}: {e}", row["slug"]));
                let binding = &bindings()[row["slug"].as_str().unwrap()];
                assert_eq!(
                    offers.keys().collect::<Vec<_>>(),
                    binding.modes.keys().collect::<Vec<_>>(),
                    "{}",
                    row["slug"]
                );
                for c in row["controls"].as_array().unwrap() {
                    let key = c["variant"].as_str().map_or_else(
                        || c["mode"].as_str().unwrap().to_owned(),
                        |v| format!("{}:{v}", c["mode"].as_str().unwrap()),
                    );
                    let offer = &offers[&key];
                    assert_eq!(offer.level, c["betlevel"].as_str().unwrap());
                    assert_eq!(offer.amount, number(c["amount"].as_str().unwrap()).unwrap());
                    count += 1;
                }
            }
        }
        assert_eq!(count, 803);
    }
    fn fixture() -> (String, String, String, Offer) {
        let init = r#"<response><method>init</method><errorcode>0</errorcode><game><paytable>&lt;SETUP&gt;&lt;MODE name="BASE"/&gt;&lt;/SETUP&gt;</paytable></game></response>"#;
        let purchase = r#"<response><method>purchase</method><errorcode>0</errorcode><roundid>r1</roundid><game><pubdata>&lt;PURCHASES&gt;&lt;PURCHASE bmode="X02" betlevel="1"&gt;&lt;RESULT mode="BASE" fsleft="0"/&gt;&lt;/PURCHASE&gt;&lt;/PURCHASES&gt;</pubdata></game></response>"#;
        let check = r#"<response><method>checkwin</method><errorcode>0</errorcode><roundid>r1</roundid></response>"#;
        (
            init.into(),
            purchase.into(),
            check.into(),
            Offer {
                level: "1".into(),
                amount: Decimal::new(2, 1),
                mode: "X02".into(),
                variant: None,
                uses_bmode: true,
            },
        )
    }
    #[test]
    fn xml_native_close_needs_no_wallet_or_prizes() {
        let (a, b, c, offer) = fixture();
        assert_eq!(
            settlement(&a, &b, &c, &offer)
                .unwrap()
                .native_round_id
                .as_deref(),
            Some("r1")
        );
    }
    #[test]
    fn optional_round_metadata_must_correlate_without_requiring_money() {
        let (a, b, c, offer) = fixture();
        let matching = b.replace("<roundid>r1</roundid>", "<roundid>r1</roundid><roundinfo><roundid>r1</roundid><bmode>X02</bmode><betlevel>1</betlevel></roundinfo>");
        assert!(settlement(&a, &matching, &c, &offer).is_ok());
        let changed = matching.replace(
            "<roundinfo><roundid>r1</roundid>",
            "<roundinfo><roundid>r2</roundid>",
        );
        assert!(
            settlement(&a, &changed, &c, &offer)
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
    #[test]
    fn xml_unknown_continuation_and_counter_are_typed() {
        let (a, b, c, offer) = fixture();
        for changed in [
            b.replace("mode=\"BASE\"", "mode=\"NOVEL\""),
            b.replace("fsleft=\"0\"", "fsleft=\"1\""),
        ] {
            let error = settlement(&a, &changed, &c, &offer).unwrap_err();
            assert!(error.downcast_ref::<crate::model::UnknownEvent>().is_some());
        }
    }
    #[test]
    fn classic_xml_and_native_boss_masks_do_not_need_money() {
        let (a, b, c, mut offer) = fixture();
        offer.uses_bmode = false;
        offer.mode = "DEF".into();
        let a = a.replace(
            "&lt;SETUP&gt;&lt;MODE name=\"BASE\"/&gt;&lt;/SETUP&gt;",
            "&lt;MODES&gt;&lt;CONFIG/&gt;&lt;/MODES&gt;",
        );
        let b = b
            .replace(" bmode=\"X02\" betlevel=\"1\"", "")
            .replace("mode=\"BASE\" fsleft=\"0\"", "freespin=\"0\"");
        assert!(settlement(&a, &b, &c, &offer).is_ok());
        let state = r#"<STATE><VAR id="collected-tokens-temple"><STATE>7</STATE></VAR><VAR id="collected-tokens-ship"><STATE>3</STATE></VAR><VAR id="collected-tokens-nest"><STATE>0</STATE></VAR></STATE>"#;
        assert_eq!(progress(state).unwrap()["ship"], 3);
        assert!(
            progress(&state.replace(">7<", ">8<"))
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
    #[test]
    fn variant_selector_is_sent_exactly_and_new_mode_never_falls_back() {
        let provider: crate::model::Provider =
            serde_json::from_str(include_str!("../../../standalone info/elk/inventario.json"))
                .unwrap();
        let game = provider.games.iter().find(|g| g.id == "pirots-5").unwrap();
        let mut mode = game.modes[0].clone();
        mode.key = "X99:boss".into();
        assert!(!supports(game, &mode));
        let (_, b, c, mut offer) = fixture();
        offer.variant = Some("boss".into());
        let a = r#"<response><method>init</method><errorcode>0</errorcode><variants><variant id="boss"><game><paytable>&lt;SETUP&gt;&lt;MODE name="BASE"/&gt;&lt;/SETUP&gt;</paytable></game></variant></variants></response>"#;
        assert!(settlement(a, &b, &c, &offer).is_err());
        assert!(
            settlement(
                a,
                &b.replace("betlevel=\"1\"", "betlevel=\"1\" variant=\"boss\""),
                &c,
                &offer
            )
            .is_ok()
        );
    }
}
