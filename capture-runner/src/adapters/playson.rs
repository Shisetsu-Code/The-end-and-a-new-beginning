//! Native capture protocol derived from standalone info/playson/codigo and the evidence reference adapter.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, now_ms, sha256, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use roxmltree::{Document, Node};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn parse_xml(s: &str) -> Result<Document<'_>> {
    ensure!(
        s.len() <= 8 * 1024 * 1024
            && !s.to_ascii_uppercase().contains("<!DOCTYPE")
            && !s.to_ascii_uppercase().contains("<!ENTITY"),
        "unsafe XML"
    );
    let d = Document::parse(s)?;
    ensure!(
        matches!(d.root_element().tag_name().name(), "server" | "client"),
        "unexpected XML root"
    );
    Ok(d)
}

fn minor(s: &str) -> Result<u64> {
    ensure!(
        !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()),
        "invalid minor amount"
    );
    Ok(s.parse()?)
}
fn child<'a>(n: Node<'a, 'a>, name: &str) -> Result<Node<'a, 'a>> {
    n.children()
        .find(|x| x.has_tag_name(name))
        .with_context(|| format!("missing {name}"))
}
fn attr<'a>(n: Node<'a, 'a>, key: &str) -> Result<&'a str> {
    n.attribute(key).with_context(|| format!("missing {key}"))
}
fn list(s: &str) -> Result<Vec<u64>> {
    s.split(',').map(minor).collect()
}
struct Quote {
    line: u64,
    buys: Vec<u64>,
    antes: Vec<Decimal>,
}
fn quote(
    d: &Document<'_>,
    _fallback: Option<u64>,
    ante_fallback: Option<Vec<String>>,
) -> Result<Quote> {
    let root = d.root_element();
    let stakes = list(
        child(child(root, "extra")?, "stakeIncrement")?
            .text()
            .context("missing stakes")?,
    )?;
    ensure!(
        !stakes.is_empty() && stakes.iter().all(|x| *x > 0),
        "invalid stake domain"
    );
    let g = child(root, "game")?;
    let buys = g
        .attribute("buy_feature_cost")
        .or_else(|| g.attribute("free_game_cost"))
        .map(list)
        .transpose()?
        .unwrap_or_default();
    ensure!(buys.iter().all(|x| *x > 0), "invalid buy domain");
    let parse_antes = |s: &str| -> Result<Vec<Decimal>> {
        if s.is_empty() {
            return Ok(vec![]);
        }
        s.split(',')
            .map(|s| {
                ensure!(
                    s.bytes().all(|c| c.is_ascii_digit() || c == b'.'),
                    "invalid ante"
                );
                let d = decimal(s)?;
                ensure!(d > Decimal::ONE, "invalid ante");
                Ok(d)
            })
            .collect()
    };
    let server = parse_antes(g.attribute("chance_booster").unwrap_or(""))?;
    let fallback = ante_fallback
        .map(|x| parse_antes(&x.join(",")))
        .transpose()?;
    if let Some(f) = &fallback {
        ensure!(server.is_empty() || server == *f, "ante source mismatch");
    }
    let antes = if server.is_empty() {
        fallback.unwrap_or_default()
    } else {
        server
    };
    Ok(Quote {
        line: *stakes.iter().min().unwrap(),
        buys,
        antes,
    })
}
fn text_field<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v.get(k)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .with_context(|| format!("missing binding.{k}"))
}

pub(super) fn validate_reviewed_mode(binding: &Value, mode: &str) -> Result<()> {
    if let Some(modes) = binding.get("verified_modes") {
        ensure!(
            modes
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item == mode)),
            "Playson mode lacks a matched native receipt: {mode}"
        );
    }
    Ok(())
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn native_title(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
struct Conn<'a> {
    ctx: &'a RunContext,
    config: Value,
    session: Option<String>,
}
impl Conn<'_> {
    async fn exchange(
        &mut self,
        command: &str,
        extra: &[(&str, String)],
        children: &str,
        financial: bool,
    ) -> Result<String> {
        let rnd = now_ms().to_string();
        let mut attrs = format!(
            "command=\"{}\" rnd=\"{}\" snd=\"1\" spn=\"0\" dwnl=\"-1\"",
            escape(command),
            rnd
        );
        if let Some(s) = &self.session {
            attrs.push_str(&format!(" session=\"{}\"", escape(s)));
        }
        for (k, v) in extra {
            attrs.push_str(&format!(" {}=\"{}\"", k, escape(v)));
        }
        let body = format!("<client {attrs}>{children}</client>");
        let mut url = url::Url::parse("https://demo.0y81pm-c-o6pywcq.com")?;
        url.set_path(command);
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("r", &rnd)
                .append_pair("gid", text_field(&self.config, "game")?)
                .append_pair("wlCode", "demomode")
                .append_pair(
                    "projectId",
                    self.config["project_id"].to_string().trim_matches('"'),
                );
        }
        let headers = BTreeMap::from([
            (
                "Content-Type".into(),
                "application/x-www-form-urlencoded; charset=UTF-8".into(),
            ),
            (
                "Origin".into(),
                "https://xplatformwl-lrs.box-int-54f2g.com".into(),
            ),
            (
                "Referer".into(),
                "https://xplatformwl-lrs.box-int-54f2g.com/".into(),
            ),
        ]);
        let response = self
            .ctx
            .http(command, "POST", url.as_str(), headers, body, financial)
            .await?;
        ensure!(response.status == 200, "native HTTP not confirmed");
        let doc = parse_xml(&response.body)?;
        let root = doc.root_element();
        ensure!(
            root.tag_name().name() == "server"
                && root.attribute("command") == Some(command)
                && root.attribute("rnd") == Some(&rnd),
            "response correlation mismatch"
        );
        ensure!(
            root.attribute("status") != Some("sessionlost"),
            "native session lost"
        );
        if let Some(existing) = &self.session {
            if let Some(s) = root.attribute("session") {
                ensure!(s == existing, "native owner changed");
            }
        } else if let Some(s) = root.attribute("session") {
            self.session = Some(s.to_owned());
        }
        drop(doc);
        Ok(response.body)
    }
}
/// `binding`: reviewed `launch_url`, `source_sha256`, `engine_version`, `debug`,
/// `slug`, `currency`, `currency_decimals`, and optionally native multiplier/ante domains and purchase style.
/// All source-derived fallback values must refer to the SHA pinned here.
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    let binding = settings
        .get("binding")
        .context("settings.binding with reviewed public source required")?;
    validate_reviewed_mode(binding, &mode.key)?;
    let use_bonus_counter = binding["bonus_round_counter"] == true
        && !binding["bare_bonus_modes"]
            .as_array()
            .is_some_and(|modes| modes.iter().any(|item| item.as_str() == Some(&mode.key)));
    let launch = text_field(binding, "launch_url")?;
    let expected_hash = text_field(binding, "source_sha256")?;
    let engine = text_field(binding, "engine_version")?;

    ensure!(
        text_field(binding, "slug")? == game.id,
        "binding slug does not match inventory game"
    );
    ensure!(
        expected_hash.len() == 64 && expected_hash.bytes().all(|c| c.is_ascii_hexdigit()),
        "invalid source hash"
    );
    let u = url::Url::parse(launch)?;
    ensure!(
        u.scheme() == "https"
            && u.host_str() == Some("xplatformwl-lrs.box-int-54f2g.com")
            && u.path() == "/launch",
        "unreviewed launch URL"
    );
    let launch_response = ctx.get("official_launch", launch).await?;
    ensure!(launch_response.status == 200, "official launch failed");
    let html = &launch_response.body;
    let re = Regex::new(r"partner:\s*")?;
    let mat = re.find(html).context("official partner config absent")?;
    let mut iter = serde_json::Deserializer::from_str(&html[mat.end()..]).into_iter::<Value>();
    let config = iter.next().context("partner config missing")??;
    let testkey = text_field(&config, "key")?;
    ensure!(
        testkey.starts_with("TEST")
            && !testkey[4..].is_empty()
            && testkey[4..].bytes().all(|x| x.is_ascii_digit()),
        "not a TEST launch"
    );
    ensure!(
        config["wl"] == "demomode"
            && config["wlCode"] == "demomode"
            && config["server_url"] == "https://demo.0y81pm-c-o6pywcq.com",
        "not official demo configuration"
    );
    let title = text_field(&config, "title")?;
    let expected_title = binding
        .get("expected_native_title")
        .and_then(Value::as_str)
        .unwrap_or(&game.name);
    ensure!(
        native_title(title) == native_title(expected_title),
        "native title mismatch"
    );
    let native_game = text_field(&config, "game")?;
    ensure!(
        !native_game.contains('/') && !native_game.is_empty(),
        "unsafe game identity"
    );
    if let Some(expected_game) = binding.get("native_game") {
        ensure!(expected_game == native_game, "Playson native game changed");
    }
    let base_re = Regex::new(r#"<base\s+[^>]*href\s*=\s*["']([^"']+)["']"#)?;
    let base = base_re
        .captures(html)
        .context("game asset base absent")?
        .get(1)
        .unwrap()
        .as_str();
    let base = url::Url::parse(base)?;
    let script_re = Regex::new(r#"gamescript:\s*"(game\.[a-zA-Z0-9]+\.js)""#)?;
    let script = script_re
        .captures(html)
        .context("native game script absent")?
        .get(1)
        .unwrap()
        .as_str();
    let asset = base.join(script)?;
    ensure!(
        asset.scheme() == "https"
            && asset.host_str() == Some("games.box-int-54f2g.com")
            && asset.path().contains(&format!("/{native_game}/")),
        "native source owner mismatch"
    );
    let source = ctx.get("native_source", asset.as_str()).await?;
    ensure!(source.status == 200, "public client source HTTP error");
    if !(sha256(source.body.as_bytes()).eq_ignore_ascii_case(expected_hash)) {
        return Err(unknown_event("native source hash changed"));
    }
    if binding.get("verified_modes").is_some() && binding["native_multiplier"].is_u64() {
        let script = Regex::new(r#"<script\b[^>]*\bsrc=["'](launcher\.[a-zA-Z0-9]+\.js)["']"#)?;
        let launcher = script
            .captures(html)
            .context("native launcher script absent")?;
        let url = base.join(&launcher[1])?;
        let expected = text_field(binding, "launcher_sha256")?;
        let source = ctx.get("native_launcher_source", url.as_str()).await?;
        ensure!(source.status == 200, "public client source HTTP error");
        if !(sha256(source.body.as_bytes()).eq_ignore_ascii_case(expected)) {
            return Err(unknown_event("Playson launcher source hash changed"));
        }
    }
    let debug = binding
        .get("debug")
        .and_then(Value::as_object)
        .context("binding.debug object required")?;
    let mut debug_xml = String::from("<debug");
    for (k, v) in debug {
        ensure!(
            !k.is_empty() && k.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
            "invalid debug attribute"
        );
        let val = v
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string());
        debug_xml.push_str(&format!(" {}=\"{}\"", k, escape(&val)));
    }
    debug_xml.push_str("/>");
    let mut c = Conn {
        ctx,
        config: config.clone(),
        session: None,
    };
    let connected = c
        .exchange(
            "connect",
            &[
                ("playerguid", testkey.into()),
                ("gameid", native_game.into()),
                ("platform", "desk".into()),
                ("engine_type", "unicore".into()),
                ("engine_version", engine.into()),
                ("wl", "demomode".into()),
                ("iif", "1".into()),
                ("ourl", "https://playson.com/".into()),
                (
                    "furl",
                    "https://xplatformwl-lrs.box-int-54f2g.com/launch".into(),
                ),
            ],
            &debug_xml,
            false,
        )
        .await?;
    let d = parse_xml(&connected)?;
    let user = child(d.root_element(), "user")?;
    ensure!(
        user.attribute("is_test") == Some("true")
            && user.attribute("wl_code_id") == Some("demomode")
            && c.session.is_some(),
        "TEST session unconfirmed"
    );
    // Reconnect is state-changing in this demo; it is performed once only, before a paid entry.
    ctx.save_session(&json!({"provider":"playson","game":game.id,"native_game":native_game,"session":c.session,"config":config,"source_sha256":expected_hash}))?;
    let initial = c.exchange("reconnect", &[], "", true).await?;
    let initial_doc = parse_xml(&initial)?;
    let root = initial_doc.root_element();
    ensure!(
        root.attribute("status") == Some("ok")
            && attr(child(root, "state")?, "current_state")? == "idle",
        "demo not idle"
    );
    let fallback = binding.get("native_multiplier").and_then(Value::as_u64);
    let antes = binding
        .get("native_ante_multipliers")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string())
                })
                .collect()
        });
    let q = quote(&initial_doc, fallback, antes)?;
    let (native_mode, target_choice) = if let Some((base, raw)) = mode.key.split_once("/Choice:") {
        let target = minor(raw)?;
        let declared = binding["extra_choice_modes"]
            .as_array()
            .context("source-bound choice variants required")?
            .iter()
            .any(|v| {
                v["mode"] == mode.key
                    && v["base_mode"] == base
                    && v["target_choice"].as_u64() == Some(target)
            });
        ensure!(
            declared && binding["choice_action"] == "game",
            "choice variant not source bound"
        );
        (base, Some(target))
    } else {
        (mode.key.as_str(), None)
    };

    let mut child_xml = format!("<bet cash=\"{}\"/>", q.line);
    let mut buy_index = None;
    let mut ante_value = None;
    if native_mode == "Spin" {
        ensure!(
            mode.kind == ModeKind::Normal,
            "mode classification mismatch"
        );
    } else if let Some(raw) = native_mode.strip_prefix("Buy:") {
        ensure!(
            mode.kind == ModeKind::Purchase && binding["observed_purchase_binding"] == true,
            "purchase source binding required"
        );
        let i = minor(raw)? as usize;
        q.buys.get(i).context("purchase not offered")?;
        let style = binding
            .get("purchase_style")
            .and_then(Value::as_str)
            .unwrap_or("typed");
        child_xml.push_str(&match style {
            "typed" => format!("<buy freegame=\"true\" type=\"{i}\"/>"),
            "fixed_zero" if i == 0 && q.buys.len() == 1 => {
                "<buy freegame=\"true\" type=\"0\"/>".into()
            }
            "untyped_freegame" if i == 0 && q.buys.len() == 1 => "<buy freegame=\"true\"/>".into(),
            "untyped_bonus_game" if i == 0 && q.buys.len() == 1 => {
                "<buy bonus_game=\"true\"/>".into()
            }
            _ => bail!("unreviewed purchase style"),
        });
        buy_index = Some(i);
    } else if let Some(raw) = native_mode.strip_prefix("Ante:") {
        ensure!(
            mode.kind == ModeKind::AnteBet && binding["observed_ante_binding"] == true,
            "ante source binding required"
        );
        let n = decimal(raw)?;
        ensure!(q.antes.contains(&n), "ante not offered");
        child_xml.push_str(&format!("<chance_booster type=\"{}\"/>", escape(raw)));
        ante_value = Some(n);
    } else {
        bail!("unsupported mode or conditional parent-dependent variant")
    }

    let mut body = c
        .exchange(
            "bet",
            &[("_btn", "0".into()), ("_auto", "0".into())],
            &child_xml,
            true,
        )
        .await?;
    let mut packets = Vec::new();
    let mut kind = "paid_entry";

    let mut first = true;
    let mut bonus_counter = 0;

    let mut target_seen = false;
    for step in 0..=ctx.max_steps() {
        let doc = parse_xml(&body)?;
        let root = doc.root_element();
        let (state, status) = native_state(root)?;
        let bonus = root.children().find(|n| n.has_tag_name("bonus"));
        let g = play_receipt_game(root, kind, bonus.is_some())?;
        if first {
            let g = g.context("native paid entry missing game data")?;
            if let Some(i) = buy_index {
                let unique = q.buys.iter().filter(|&&x| x == q.buys[i]).count() == 1;
                let marker = g.attribute("bonus_game_type");
                let subtype = g.attribute("bonus_game_sub_type");
                ensure!(
                    (marker == Some("bought") || (marker.is_none() && unique))
                        && (subtype == Some(i.to_string().as_str())
                            || (subtype.is_none() && unique)),
                    "purchase response binding missing"
                );
                ensure!(
                    matches!(state, "choice" | "respin" | "bonus" | "fs"),
                    "purchase not accepted in reviewed state"
                );
            }
            if let Some(a) = ante_value {
                let x = g
                    .attribute("chance_booster_type")
                    .or_else(|| {
                        if matches!(g.attribute("ante_bet"), Some("true" | "1")) {
                            g.attribute("ante_bet_multiplier")
                        } else {
                            None
                        }
                    })
                    .context("ante response binding missing")?;
                ensure!(decimal(x)? == a, "ante response mismatch");
            }
            first = false;
        }
        packets.push(json!({"kind":kind,"xml":body.clone()}));
        if state == "idle" && matches!(status, "ok" | "bet") {
            ensure!(
                target_choice.is_none() || target_seen,
                "requested choice was not offered"
            );
            return Ok(Completion {
                native_round_id: None,
                details: json!({"initial":initial,"packets":packets,"minor_units":true}),
            });
        }
        ensure!(
            step < ctx.max_steps(),
            "native round exceeded continuation limit"
        );
        let (command, next_kind, children, selected) = if state == "choice" {
            ensure!(
                binding["choice_action"] == "game",
                "unreviewed choice action"
            );
            let offered = root
                .descendants()
                .find(|x| x.has_tag_name("player_choice"))
                .context("choice absent")?
                .attribute("types")
                .context("choice domain absent")?;
            let options = list(offered)?;
            ensure!(
                !options.is_empty() && options.len() <= 10,
                "choice domain invalid"
            );
            let selected = target_choice.unwrap_or(options[0]);
            ensure!(options.contains(&selected), "requested choice not offered");
            if target_choice.is_some() {
                target_seen = true;
            }
            (
                "game",
                "player_choice",
                format!(
                    "<bet cash=\"{}\"/><player_choice type=\"{}\"/>",
                    q.line, selected
                ),
                Some(selected),
            )
        } else if status == "next" {
            (
                "next",
                "cascade_next",
                format!("<bet cash=\"{}\"/>", q.line),
                None,
            )
        } else if state == "bonus" && matches!(status, "bonus" | "ok") {
            bonus_counter += 1;
            let extra = if use_bonus_counter {
                format!("<action round=\"{bonus_counter}\"/>")
            } else {
                String::new()
            };
            ("bonus", "bonus", extra, None)
        } else if state == "fs"
            && minor(
                g.and_then(|node| node.attribute("bonus_games"))
                    .unwrap_or("0"),
            )? > 0
        {
            (
                "bet",
                "free_bet",
                format!("<bet cash=\"{}\"/>", q.line),
                None,
            )
        } else {
            return Err(unknown_event("unreviewed native continuation"));
        };
        body = c
            .exchange(
                command,
                &[("_btn", "0".into()), ("_auto", "0".into())],
                &children,
                true,
            )
            .await?;
        if let Some(sel) = selected {
            let doc = parse_xml(&body)?;
            let echo = child(doc.root_element(), "game")?
                .attribute("player_choice_type")
                .and_then(|v| minor(v).ok());
            if echo != Some(sel) {
                return Err(unknown_event("Playson choice echo differs from request"));
            }
        }
        kind = next_kind;
    }
    bail!("continuation limit reached with root retained")
}

fn play_receipt_game<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    kind: &str,
    has_bonus: bool,
) -> Result<Option<roxmltree::Node<'a, 'input>>> {
    let game = root.children().find(|node| node.has_tag_name("game"));
    ensure!(
        game.is_some() || (kind == "bonus" && has_bonus),
        "native play receipt missing game or bonus data"
    );
    Ok(game)
}

pub fn supports(_game: &Game, mode: &Mode) -> bool {
    match mode.kind {
        ModeKind::Normal => mode.key == "Spin",
        ModeKind::Purchase => mode
            .key
            .split("/Choice:")
            .next()
            .and_then(|s| s.strip_prefix("Buy:"))
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())),
        ModeKind::AnteBet => mode.key.starts_with("Ante:"),
        _ => false,
    }
}

fn native_state<'a, 'input>(root: roxmltree::Node<'a, 'input>) -> Result<(&'a str, &'a str)> {
    let state = attr(child(root, "state")?, "current_state")?;
    let status = attr(root, "status")?;
    if !matches!(state, "idle" | "choice" | "respin" | "bonus" | "fs")
        || !matches!(status, "ok" | "bet" | "next" | "bonus")
    {
        return Err(unknown_event(format!(
            "Playson new native state/status: {state}/{status}"
        )));
    }
    Ok((state, status))
}
#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn terminal_state_parses_without_wallet_or_awards() {
        let doc = parse_xml(r#"<server status="ok"><state current_state="idle"/><game/></server>"#)
            .unwrap();
        assert_eq!(native_state(doc.root_element()).unwrap(), ("idle", "ok"));
    }
    #[test]
    fn new_native_state_is_typed() {
        let doc = parse_xml(r#"<server status="ok"><state current_state="new_feature"/></server>"#)
            .unwrap();
        assert!(
            native_state(doc.root_element())
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
