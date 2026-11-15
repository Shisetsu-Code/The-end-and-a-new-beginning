//! Native capture protocol derived from standalone info/pragmatic/codigo and the evidence reference adapter.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::Url;
type Fields = BTreeMap<String, String>;

fn parse_fields(body: &str) -> Result<Fields> {
    ensure!(
        !body.trim_start().starts_with(['{', '<']),
        "invalid provider response format"
    );
    let mut out = Fields::new();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        ensure!(
            !key.is_empty() && out.insert(key.into_owned(), value.into_owned()).is_none(),
            "duplicate or empty native field"
        );
    }
    ensure!(
        !out.contains_key("unlogged") && !(out.contains_key("frozen") && !out.contains_key("na")),
        "demo session unavailable"
    );
    ensure!(
        out.get("error")
            .is_none_or(|v| v.is_empty() || v == "0" || v.eq_ignore_ascii_case("false")),
        "provider error"
    );
    Ok(out)
}
fn number(s: &str) -> Result<Decimal> {
    ensure!(
        Regex::new(r"^(?:[0-9]+|[0-9]{1,3}(?:,[0-9]{3})+)(?:\.[0-9]+)?$")?.is_match(s),
        "invalid native money"
    );
    let normalized = s.replace(',', "");
    let v = decimal(&normalized)?;
    ensure!(v >= Decimal::ZERO, "negative amount");
    Ok(v)
}
fn field_amount(m: &Fields, k: &str) -> Result<Decimal> {
    number(
        m.get(k)
            .with_context(|| format!("missing native field {k}"))?,
    )
}
fn revision(source: &str) -> Option<String> {
    let patterns = [
        r#"UHT_REVISION\s*=\s*\{([^}]+)\}"#,
        r#"UHT_REVISION\.common\s*=\s*["'](\d+)["']"#,
    ];
    let mut found: Vec<(usize, String)> = Vec::new();
    let blocks = Regex::new(patterns[0]).ok()?;
    let common = Regex::new(r#"["']?common["']?\s*:\s*["'](\d+)["']"#).ok()?;
    for cap in blocks.captures_iter(source) {
        if let Some(v) = common.captures(&cap[1]) {
            found.push((cap.get(0)?.start(), v[1].to_string()));
        }
    }
    for cap in Regex::new(patterns[1]).ok()?.captures_iter(source) {
        found.push((cap.get(0)?.start(), cap[1].to_string()));
    }
    found.into_iter().max_by_key(|v| v.0).map(|v| v.1)
}
fn js_data(raw: &str) -> Result<Value> {
    let re = Regex::new(r"([\{,]\s*)([A-Za-z_][A-Za-z0-9_]*)\s*:")?;
    Ok(serde_json::from_str(&re.replace_all(raw, "$1\"$2\":"))?)
}
fn selector(key: &str, prefix: &str) -> Result<usize> {
    let value = key
        .strip_prefix(prefix)
        .context("unsupported inventory selector")?;
    ensure!(
        !value.is_empty()
            && value.bytes().all(|b| b.is_ascii_digit())
            && (value == "0" || !value.starts_with('0')),
        "invalid native selector"
    );
    Ok(value.parse()?)
}
fn select_mode(initial: &Fields, key: &str) -> Result<Option<(String, String)>> {
    if key == "normal" {
        return Ok(None);
    }
    if key.starts_with("pur-") {
        let index = selector(key, "pur-")?;
        let options = js_data(initial.get("purInit").context("no published purchases")?)?;
        options
            .as_array()
            .context("invalid purchase menu")?
            .get(index)
            .context("purchase not offered")?;
        if let Some(flags) = initial.get("purInit_e").filter(|s| !s.is_empty()) {
            ensure!(
                flags.split(',').nth(index) == Some("1"),
                "purchase disabled"
            );
        }
        return Ok(Some(("pur".into(), index.to_string())));
    }
    if key.starts_with("bl-") {
        let index = selector(key, "bl-")?;
        initial
            .get("bls")
            .context("bet levels not offered")?
            .split(',')
            .nth(index)
            .filter(|s| !s.is_empty())
            .context("bet level not offered")?;
        ensure!(
            initial.get("bl").map(String::as_str) != Some(index.to_string().as_str()),
            "selected bet level is default"
        );
        return Ok(Some(("bl".into(), index.to_string())));
    }
    bail!("unsupported independent mode")
}
fn int_field(m: &Fields, key: &str) -> Result<i64> {
    m.get(key)
        .map(|v| v.parse().with_context(|| format!("invalid {key}")))
        .unwrap_or(Ok(0))
}
fn active_free(m: &Fields) -> Result<bool> {
    let left = int_field(m, "fsleft")?;
    let total = int_field(m, "fs_total")?;
    let switching = int_field(m, "fsc_sw")?;
    let fs = int_field(m, "fs")?;
    let max = int_field(m, "fsmax")?;
    ensure!(
        [left, total, switching, fs, max].iter().all(|v| *v >= 0),
        "invalid free-spin count"
    );
    if left > 0 || ((total == 0 || switching > 0) && fs > 0 && max > 0 && fs <= max) {
        return Ok(true);
    }
    let lists: Vec<Vec<&str>> = ["rs_t", "rs_c", "rs_m"]
        .iter()
        .map(|k| {
            m.get(*k)
                .map(|s| s.split(',').collect())
                .unwrap_or_default()
        })
        .collect();
    for i in 0..lists.iter().map(Vec::len).max().unwrap_or(0) {
        let mut vals = [0i64; 3];
        for j in 0..3 {
            if let Some(v) = lists[j].get(i).filter(|v| !v.is_empty()) {
                vals[j] = v.parse()?;
                ensure!(vals[j] >= 0, "invalid respin count");
            }
        }
        if vals[0] == 0 && vals[1] > 0 && vals[1] <= vals[2] {
            return Ok(true);
        }
    }
    Ok(false)
}
fn bonus_choice(m: &Fields) -> Result<Option<usize>> {
    if let Some(raw) = m.get("g") {
        let data = js_data(raw)?;
        let entries = data.as_object().context("unknown bonus menu")?;
        for block in entries.values().filter(|v| v.is_object()) {
            if block
                .get("end")
                .is_some_and(|v| v.to_string().trim_matches('"') == "1")
            {
                continue;
            }
            let Some(raw_labels) = block.get("ch_k") else {
                continue;
            };
            let text = |v: &Value| -> String {
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string())
            };
            let mut labels = text(raw_labels);
            let mut values = block.get("ch_v").map(text).unwrap_or_default();
            if let Some(stage) = block.get("ask") {
                let stage = text(stage).parse::<usize>()?;
                labels = labels
                    .split(';')
                    .nth(stage)
                    .context("bonus stage missing")?
                    .to_string();
                values = values
                    .split(';')
                    .nth(stage)
                    .context("bonus stage values missing")?
                    .to_string();
            }
            let labels: Vec<_> = labels.split(',').filter(|s| !s.is_empty()).collect();
            if labels.is_empty() {
                continue;
            }
            let status = block.get("status").map(text).unwrap_or_default();
            let statuses: Vec<_> = status.split(',').collect();
            let indices: Vec<usize> = if labels == ["pck"] {
                let range: Vec<_> = values.split(':').collect();
                ensure!(range.len() == 2, "invalid pick range");
                let lo: usize = range[0].parse()?;
                let hi: usize = range[1].parse()?;
                ensure!(
                    lo <= hi && hi < 128 && statuses.len() == hi - lo + 1,
                    "invalid published pick range"
                );
                (lo..=hi).collect()
            } else {
                (0..labels.len()).collect()
            };
            let history = block.get("ch_h").map(text).unwrap_or_default();
            for (pos, index) in indices.iter().enumerate() {
                if !status.is_empty() && statuses.len() == indices.len() && statuses[pos] != "0" {
                    continue;
                }
                if status.is_empty()
                    && history.split([';', ',']).any(|x| {
                        let h = x.split('~').next().unwrap_or("");
                        h == index.to_string() || labels.get(pos).is_some_and(|label| *label == h)
                    })
                {
                    continue;
                }
                return Ok(Some(*index));
            }
        }
        bail!("no verified available bonus option");
    }
    if let Some(status) = m.get("status").filter(|s| s.contains(',')) {
        let values: Vec<_> = status.split(',').collect();
        ensure!(
            values
                .iter()
                .all(|s| !s.is_empty() && s.len() <= 3 && s.bytes().all(|b| b.is_ascii_digit())),
            "invalid legacy bonus options"
        );
        return Ok(Some(
            values
                .iter()
                .position(|s| *s == "0")
                .context("no available bonus option")?,
        ));
    }
    bail!("bonus choice not published")
}
fn next_step(m: &Fields, previous: Option<&str>) -> Result<Option<(&'static str, Option<usize>)>> {
    let actions: Vec<_> = m
        .get("na")
        .ok_or_else(|| unknown_event("missing native next action"))?
        .split(',')
        .collect();
    if !(!actions.is_empty()
        && actions.iter().all(|a| {
            matches!(
                *a,
                "s" | "c" | "b" | "fso" | "m" | "cb" | "fsb" | "cj" | "pj"
            )
        }))
    {
        return Err(unknown_event("unrecognized native next-action vocabulary"));
    }
    if actions.iter().any(|a| matches!(*a, "fsb" | "cj" | "pj")) {
        return Err(unknown_event(
            "Pragmatic continuation has no implemented binding",
        ));
    }
    if actions.contains(&"c") {
        return Ok(Some(("doCollect", None)));
    }
    if actions.contains(&"b") {
        if m.contains_key("rsb_c") || m.contains_key("rsb_m") {
            return Ok(Some(("doBonus", None)));
        }
        return Ok(Some(("doBonus", bonus_choice(m)?)));
    }
    if actions.contains(&"fso") {
        let options = m
            .get("fs_opt")
            .filter(|s| !s.is_empty())
            .context("free-spin selection not published")?;
        ensure!(
            !options.split(',').next().unwrap_or("").is_empty(),
            "empty free-spin option"
        );
        return Ok(Some(("doFSOption", Some(0))));
    }
    if actions.contains(&"m") {
        return Ok(Some(("doMysteryScatter", None)));
    }
    if actions.contains(&"cb") {
        return Ok(Some(("doCollectBonus", None)));
    }
    if actions.iter().any(|a| matches!(*a, "fsb" | "cj" | "pj")) {
        return Err(unknown_event("unrecognized continuation"));
    }
    if actions.contains(&"s") {
        if active_free(m)? || (previous == Some("doMysteryScatter") && m.contains_key("ms")) {
            return Ok(Some(("doSpin", None)));
        }
        return Ok(None);
    }
    Err(unknown_event("unrecognized native next action"))
}

fn demo_url(u: &Url) -> bool {
    u.scheme() == "https"
        && u.host_str() == Some("demogamesfree.pragmaticplay.net")
        && u.port().is_none()
        && u.username().is_empty()
        && u.password().is_none()
}
async fn launch(ctx: &RunContext, game: &str) -> Result<(Url, String)> {
    let mut url = Url::parse("https://demogamesfree.pragmaticplay.net/gs2c/openGame.do")?;
    url.query_pairs_mut()
        .append_pair("gameSymbol", game)
        .append_pair("websiteUrl", "https://demogamesfree.pragmaticplay.net")
        .append_pair("jurisdiction", "99")
        .append_pair("lang", "en")
        .append_pair("cur", "USD");
    for _ in 0..10 {
        ensure!(
            demo_url(&url),
            "launcher redirected outside official demo host"
        );
        let response = ctx.get("pragmatic_launch", url.as_str()).await?;
        if (300..400).contains(&response.status) {
            let location = response
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("location"))
                .map(|(_, v)| v)
                .context("launch redirect without location")?;
            url = url.join(location)?;
            continue;
        }
        ensure!((200..300).contains(&response.status), "launch HTTP error");
        return Ok((url, response.body));
    }
    bail!("too many launcher redirects")
}
fn html_url(source: &str) -> String {
    source
        .replace("&amp;", "&")
        .replace("&#38;", "&")
        .replace("&#x26;", "&")
}
async fn discover_version(
    ctx: &RunContext,
    document: &str,
    document_url: &Url,
    symbol: &str,
) -> Result<String> {
    let src_re = Regex::new(r#"(?i)<script\b[^>]*\bsrc\s*=\s*["']([^"']+)["']"#)?;
    let mut urls = Vec::new();
    for cap in src_re.captures_iter(document) {
        if let Ok(url) = document_url.join(&html_url(&cap[1])) {
            urls.push(url);
        }
    }
    if Regex::new(r"^(?:vs|cs|sc)[A-Za-z0-9_]+$")?.is_match(symbol) {
        urls.push(Url::parse(&format!("https://demogamesfree.pragmaticplay.net/gs2c/common/v3/games-html5/games/{}/{symbol}/desktop/bootstrap.js",&symbol[..2]))?);
    }
    let cdn = Regex::new(r"^demogamesfree(?:-[a-z0-9-]+)?\.pragmaticplay\.net$")?;
    for url in urls {
        if url.scheme() != "https"
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || !cdn.is_match(url.host_str().unwrap_or(""))
            || !url.path().starts_with("/gs2c/common/")
            || !url.path().ends_with("/bootstrap.js")
        {
            continue;
        }
        let response = ctx.get("public_bootstrap", url.as_str()).await?;
        if response.status == 404 {
            continue;
        }
        ensure!(
            (200..300).contains(&response.status),
            "bootstrap HTTP error"
        );
        if let Some(version) = revision(&response.body) {
            return Ok(version);
        }
    }
    bail!("public bootstrap did not disclose a client revision")
}
#[allow(clippy::too_many_arguments)]
async fn action(
    ctx: &RunContext,
    service: &Url,
    token: &str,
    symbol: &str,
    cver: &str,
    seq: &mut u64,
    name: &str,
    extra: &Fields,
    financial: bool,
) -> Result<Fields> {
    *seq += 1;
    let mut values = Fields::from([
        ("action".into(), name.into()),
        ("symbol".into(), symbol.into()),
        ("cver".into(), cver.into()),
        ("index".into(), seq.to_string()),
        ("counter".into(), (2 * (*seq) - 1).to_string()),
        ("repeat".into(), "0".into()),
        ("mgckey".into(), token.into()),
    ]);
    for (k, v) in extra {
        ensure!(!values.contains_key(k), "duplicate request parameter");
        values.insert(k.clone(), v.clone());
    }
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(&values)
        .finish();
    let headers = BTreeMap::from([(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    )]);
    let result = ctx
        .http(name, "POST", service.as_str(), headers, body, financial)
        .await?;
    ensure!(
        (200..300).contains(&result.status),
        "native action HTTP error"
    );
    let fields = parse_fields(&result.body)?;

    Ok(fields)
}
fn spin_parameters(
    initial: &Fields,
    symbol: &str,
    selector: Option<&(String, String)>,
    coin: Decimal,
) -> Result<Fields> {
    let mut p = Fields::from([
        ("c".into(), coin.to_string()),
        (
            "l".into(),
            initial.get("l").context("missing lines")?.clone(),
        ),
    ]);
    if let Some(bl) = initial.get("bl") {
        p.insert("bl".into(), bl.clone());
    }
    if let Some((key, value)) = selector {
        p.insert(key.clone(), value.clone());
    }
    if let Some(prg) = initial.get("prg_i") {
        ensure!(
            !prg.is_empty() && prg.bytes().all(|b| b.is_ascii_digit()),
            "invalid progressive selector"
        );
        p.insert("prg_i".into(), prg.clone());
    }
    if ["vs15raos", "vs15godsofwar", "vs15zeushadseq"].contains(&symbol) {
        if symbol == "vs15raos" {
            ensure!(
                initial.get("i_ch_k").map(String::as_str) == Some("r,o")
                    && initial.get("i_ch_v").map(String::as_str) == Some("0,1")
                    && initial.get("i_ask").map(String::as_str) == Some("0"),
                "unrecognized Ra/Osiris selection"
            );
        } else {
            let info = initial
                .get("gameInfo")
                .context("missing god selection contract")?;
            ensure!(
                info.contains("regular_200") && info.contains("regular_400"),
                "unrecognized Zeus/Hades selection"
            );
        }
        let selected = if let Some((key, value)) = selector.filter(|(k, _)| k == "pur") {
            let _ = key;
            let index: usize = value.parse()?;
            let (count, boundary) = if symbol == "vs15zeushadseq" {
                (6, 3)
            } else {
                (4, 2)
            };
            ensure!(index < count, "unsupported god purchase selector");
            if index < boundary { 0 } else { 1 }
        } else {
            0
        };
        p.insert("ind".into(), selected.to_string());
    } else if symbol == "vs10bookfallen" && selector.is_some_and(|(k, v)| k == "pur" && v == "0") {
        p.insert("ind".into(), "0".into());
    } else if initial.contains_key("i_ch_k")
        || initial.contains_key("i_ch_v")
        || initial.contains_key("i_ask")
    {
        bail!("unrecognized initial game selection");
    }
    Ok(p)
}
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    ensure!(
        Regex::new(r"^[A-Za-z0-9_-]{1,80}$")?.is_match(&game.id),
        "invalid game symbol"
    );
    match mode.kind {
        ModeKind::Normal => ensure!(mode.key == "normal", "invalid normal selector"),
        ModeKind::Purchase => {
            selector(&mode.key, "pur-")?;
        }
        ModeKind::Variant | ModeKind::AnteBet | ModeKind::Booster => {
            selector(&mode.key, "bl-")?;
        }
        _ => bail!("unsupported Pragmatic mode"),
    }
    let (document_url, document) = launch(ctx, &game.id).await?;
    let tokens: Vec<_> = document_url
        .query_pairs()
        .filter(|(k, _)| k == "mgckey")
        .map(|(_, v)| v.into_owned())
        .collect();
    ensure!(
        tokens.len() == 1 && !tokens[0].is_empty(),
        "fresh demo session missing mgckey"
    );
    let token = &tokens[0];
    let re = Regex::new(r#""gameService"\s*:\s*("[^"\r\n]+")"#)?;
    let cap = re
        .captures(&document)
        .context("native game service missing")?;
    let service_raw: String = serde_json::from_str(&cap[1])?;
    let service = document_url.join(&service_raw)?;
    ensure!(
        demo_url(&service)
            && Regex::new(r"^/gs2c/(?:ge/)?(?:v\d+/)?gameService$")?.is_match(service.path())
            && service.query().is_none()
            && service.fragment().is_none(),
        "unrecognized game service endpoint"
    );
    let cver = discover_version(ctx, &document, &document_url, &game.id).await?;
    let mut seq = 0;
    let initial = action(
        ctx,
        &service,
        token,
        &game.id,
        &cver,
        &mut seq,
        "doInit",
        &Fields::new(),
        false,
    )
    .await?;
    ensure!(
        !active_free(&initial)?,
        "fresh demo has an unfinished round"
    );
    if let Some(na) = initial.get("na") {
        ensure!(na == "s", "fresh demo has an outstanding action");
    }

    let coins = initial
        .get("sc")
        .context("missing published coins")?
        .split(',')
        .map(number)
        .collect::<Result<Vec<_>>>()?;
    let coin = coins.into_iter().min().context("empty coin offers")?;
    ensure!(
        coin > Decimal::ZERO && field_amount(&initial, "l")? > Decimal::ZERO,
        "invalid native stake"
    );
    let selection = select_mode(&initial, &mode.key)?;

    let params = spin_parameters(&initial, &game.id, selection.as_ref(), coin)?;
    ctx.save_session(&json!({"provider":"pragmatic","symbol":game.id,"mgckey":token,"service":service.as_str(),"cver":cver,"currency_requested":"USD"}))?;
    let mut current = action(
        ctx, &service, token, &game.id, &cver, &mut seq, "doSpin", &params, true,
    )
    .await?;
    let mut snapshots = vec![current.clone()];
    let mut previous = "doSpin";
    let mut closed = false;
    for step in 0..=ctx.max_steps() {
        let Some((method, choice)) = next_step(&current, Some(previous))? else {
            closed = true;
            break;
        };
        ensure!(
            step < ctx.max_steps(),
            "native round exceeded continuation limit"
        );
        let mut extra = if method == "doSpin" {
            let mut continuation = params.clone();
            continuation.remove("pur");
            continuation
        } else {
            Fields::new()
        };
        if let Some(index) = choice {
            extra.insert("ind".into(), index.to_string());
        }
        current = action(
            ctx, &service, token, &game.id, &cver, &mut seq, method, &extra, true,
        )
        .await?;
        snapshots.push(current.clone());
        previous = method;
    }
    ensure!(closed, "native round exceeded continuation limit");
    Ok(Completion {
        native_round_id: None,
        details: json!({"initial":initial,"client_revision":cver,"request_parameters":params,"responses":snapshots}),
    })
}

pub fn supports(_game: &Game, mode: &Mode) -> bool {
    match mode.kind {
        ModeKind::Normal => mode.key == "normal",
        ModeKind::Purchase => selector(&mode.key, "pur-").is_ok(),
        ModeKind::AnteBet | ModeKind::Booster | ModeKind::Variant => {
            selector(&mode.key, "bl-").is_ok()
        }
        _ => false,
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn normal_and_free_round_closure_without_balance_or_payout() {
        assert_eq!(
            next_step(&parse_fields("na=s").unwrap(), None).unwrap(),
            None
        );
        assert_eq!(
            next_step(&parse_fields("na=c").unwrap(), None).unwrap(),
            Some(("doCollect", None))
        );
        assert_eq!(
            next_step(&parse_fields("na=s&fs=1&fsmax=3").unwrap(), None).unwrap(),
            Some(("doSpin", None))
        );
    }
    #[test]
    fn mixed_known_and_unknown_control_is_typed_novelty() {
        for raw in ["na=s,unknown", "na=c,fsb", "na=new_control"] {
            let err = next_step(&parse_fields(raw).unwrap(), None).unwrap_err();
            assert!(err.downcast_ref::<crate::model::UnknownEvent>().is_some());
        }
    }
}
