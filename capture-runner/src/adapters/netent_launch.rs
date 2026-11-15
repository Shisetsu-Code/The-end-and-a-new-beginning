//! Native capture protocol from standalone info/netent/codigo/netent_launch.rs.
//! Published evidence SHA-256: 9bd6ecbe8cf1709dd4e04b582a0ec497a9b7d246b10dcb89d7820e98d74a75d3
//! Reference transport game-runner/src/adapters/netent_launch.rs; no runtime dependency.
use crate::context::RunContext;
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::collections::BTreeMap;
use url::Url;

const ORIGIN: &str = "https://games.evolution.com";
const SHOWCASE: &str = "https://showcase.evo-games.com";
const CLIENT_VERSION: &str = "6.20261030.73038.67000-7e7b03116d-r2";

fn demo_wallet(value: &str) -> bool {
    value.strip_prefix("demo-").is_some_and(|id| {
        !id.is_empty()
            && id.len() <= 256
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'='))
    })
}

fn demo_fragment(url: &Url) -> Option<(String, String)> {
    let mut wallet = None;
    let mut table = None;
    for (key, value) in url::form_urlencoded::parse(url.fragment()?.as_bytes()) {
        match key.as_ref() {
            "balance_id" if wallet.is_none() && demo_wallet(&value) => {
                wallet = Some(value.into_owned());
            }
            "table_id"
                if table.is_none()
                    && !value.is_empty()
                    && value.len() <= 64
                    && value.bytes().all(|b| b.is_ascii_alphanumeric()) =>
            {
                table = Some(value.into_owned());
            }
            "balance_id" | "table_id" => return None,
            _ => {}
        }
    }
    wallet.zip(table)
}

fn geco_init_url(config: &Value, table: &str, wallet: &str) -> Result<Url> {
    ensure!(
        demo_wallet(wallet)
            && config["table_id"] == table
            && config["game_provider"] == "netent"
            && config["game_engine"] == "geco"
            && config["userType"] == 0,
        "NetEnt config did not confirm the launched GECO demo table"
    );
    let launch = &config["wrappedLaunchParams"];
    let required = |name: &str| -> Result<&str> {
        launch[name]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 4096 && !v.chars().any(char::is_control))
            .with_context(|| format!("GECO launch parameter {name} missing or malformed"))
    };
    let session = required("sessionId")?;
    let game = required("gameId")?;
    let currency = required("ne_currency")?;
    ensure!(
        session.len() <= 128
            && session.bytes().all(|b| b.is_ascii_alphanumeric())
            && config["bare_session_id"] == session
            && game
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            && currency.len() == 3
            && currency.bytes().all(|b| b.is_ascii_uppercase())
            && launch["ne_device"] == "desktop"
            && launch["gameServer"] == format!("{SHOWCASE}/public/rng/dragon"),
        "GECO launch parameters disagree with the active demo"
    );
    let mut url = Url::parse(&format!(
        "{SHOWCASE}/public/rng/dragon/servlet/CasinoGameServlet;jsession={session}"
    ))?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("action", "init")
            .append_pair("gameId", game)
            .append_pair("sessid", session)
            .append_pair("ne_balance_id", wallet)
            .append_pair("ne_mode", "demo")
            .append_pair("wantsfreerounds", "true")
            .append_pair("freeroundmode", "false")
            .append_pair("wantsreels", "true")
            .append_pair("no-cache", &crate::model::now_ms().to_string());
        for key in [
            "ne_currency",
            "ne_casinoId",
            "ne_device",
            "ne_evo_token",
            "ne_evo_token1",
            "ne_evo_token2",
        ] {
            query.append_pair(key, required(key)?);
        }
    }
    Ok(url)
}

fn validate_entry_target(url: &Url, origin: &str, hop: usize) -> Result<()> {
    ensure!(
        url.origin().ascii_serialization() == origin
            && url.username().is_empty()
            && url.password().is_none()
            && ((url.path() == "/entry" && url.fragment().is_none())
                || (hop > 0 && url.path().starts_with("/frontend/evo/"))),
        "unexpected NetEnt demo redirect"
    );
    Ok(())
}

#[derive(serde::Deserialize)]
struct Launch {
    family: String,
    public_game_id: String,
}

fn nonce(html: &str) -> Result<String> {
    let script = regex::Regex::new(r"var\s+game_api_settings\s*=\s*(\{[^;]+\});")?;
    let mut scripts = script.captures_iter(html);
    let entry = scripts
        .next()
        .context("NetEnt public launch settings missing")?;
    ensure!(scripts.next().is_none(), "NetEnt launch settings ambiguous");
    let settings: Value = serde_json::from_str(&entry[1])?;
    let nonce = settings["nonce"]
        .as_str()
        .filter(|v| (8..=64).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_alphanumeric()))
        .context("NetEnt launch nonce missing or malformed")?;
    Ok(nonce.to_owned())
}

pub(crate) struct DemoLaunch {
    pub config: Value,
    pub wallet: String,
    pub frontend: Url,
    pub frontend_body: String,
}

pub(crate) async fn open_config(
    ctx: &RunContext,
    game: &str,
    table: &str,
    family: &str,
    client_version: &str,
) -> Result<DemoLaunch> {
    let launches: BTreeMap<String, Launch> =
        serde_json::from_slice(include_bytes!("../../bindings/netent-launch.json"))?;
    let row = launches
        .get(game)
        .context("missing reviewed NetEnt launcher")?;
    ensure!(
        row.family == family
            && !row.public_game_id.is_empty()
            && row.public_game_id.bytes().all(|b| b.is_ascii_digit())
            && game
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "invalid NetEnt launch binding"
    );
    let page = format!("{ORIGIN}/slots/{game}/");
    let reply = ctx.get("netent_game_page", &page).await?;
    ensure!(
        reply.status == 200,
        "HTTP {} at NetEnt game page",
        reply.status
    );
    let mut issuer = Url::parse(&format!("{ORIGIN}/wp-json/games/v1/start"))?;
    issuer
        .query_pairs_mut()
        .append_pair("id", &row.public_game_id)
        .append_pair("mobile", "false");
    ctx.seed_cookie("age_confirmed=1; Path=/; Secure", &page)?;
    let headers = BTreeMap::from([
        ("accept".into(), "*/*".into()),
        ("referer".into(), page.clone()),
        ("x-requested-with".into(), "XMLHttpRequest".into()),
        ("x-wp-nonce".into(), nonce(&reply.body)?),
    ]);
    let reply = ctx
        .http(
            "netent_demo_issuer",
            "GET",
            issuer.as_str(),
            headers,
            String::new(),
            false,
        )
        .await?;
    ensure!(
        reply.status == 200,
        "HTTP {} at NetEnt demo issuer",
        reply.status
    );
    let launch: Value =
        serde_json::from_str(&reply.body).context("invalid NetEnt issuer response")?;
    ensure!(
        launch["success"] == true,
        "NetEnt demo issuer declined the launch"
    );
    let mut entry = launch["payload"]
        .as_str()
        .and_then(|s| Url::parse(s).ok())
        .context("invalid NetEnt issuer entry")?;
    for hop in 0..4 {
        validate_entry_target(&entry, SHOWCASE, hop)?;
        let mut frontend = entry.clone();
        frontend.set_fragment(None);
        let reply = ctx
            .http(
                "netent_demo_entry",
                "GET",
                frontend.as_str(),
                BTreeMap::from([("referer".into(), page.clone())]),
                String::new(),
                false,
            )
            .await?;
        if matches!(reply.status, 301 | 302 | 303 | 307 | 308) {
            let location = reply
                .headers
                .get("location")
                .context("NetEnt entry redirect missing")?;
            entry = entry
                .join(location)
                .map_err(|_| anyhow::anyhow!("invalid NetEnt entry redirect"))?;
            continue;
        }
        ensure!(
            reply.status == 200 && frontend.path().starts_with("/frontend/evo/"),
            "HTTP {} at NetEnt demo entry without a supported frontend",
            reply.status
        );
        let frontend_body = reply.body;
        let (wallet, launched_table) =
            demo_fragment(&entry).context("NetEnt demo wallet missing from frontend")?;
        ensure!(
            launched_table == table,
            "NetEnt issuer returned a different demo table"
        );
        let mut config_url = Url::parse(&format!("{SHOWCASE}/config"))?;
        config_url
            .query_pairs_mut()
            .append_pair("table_id", table)
            .append_pair("client_version", client_version);
        let reply = ctx
            .http(
                "netent_demo_config",
                "GET",
                config_url.as_str(),
                BTreeMap::from([("referer".into(), frontend.to_string())]),
                String::new(),
                false,
            )
            .await?;
        ensure!(
            reply.status == 200,
            "HTTP {} at NetEnt demo config",
            reply.status
        );
        let config: Value =
            serde_json::from_str(&reply.body).context("invalid NetEnt demo config")?;
        return Ok(DemoLaunch {
            config,
            wallet,
            frontend,
            frontend_body,
        });
    }
    anyhow::bail!("NetEnt demo entry redirect limit reached")
}

pub(crate) async fn open_geco(
    ctx: &RunContext,
    game: &str,
    table: &str,
    native_game: &str,
    asset: Option<(&str, &str)>,
) -> Result<(Url, String)> {
    let DemoLaunch {
        config,
        wallet,
        frontend,
        ..
    } = open_config(ctx, game, table, "geco_http", CLIENT_VERSION).await?;
    ensure!(
        config["wrappedLaunchParams"]["gameId"] == native_game,
        "NetEnt native game identity changed"
    );
    let init = geco_init_url(&config, table, &wallet)?;
    if let Some((path, digest)) = asset {
        const STATIC: &str = "https://affiliate0000001-static2.casinomodule.com/";
        ensure!(
            config["wrappedLaunchParams"]["staticServer"] == STATIC && path.starts_with("/games/"),
            "NetEnt GECO asset origin changed"
        );
        let source = Url::parse(STATIC)?.join(path)?;
        let reply = ctx
            .get("netent_geco_reviewed_source", source.as_str())
            .await?;
        if !(reply.status == 200 && crate::model::sha256(reply.body.as_bytes()) == digest) {
            return Err(crate::model::unknown_event(
                "NetEnt GECO reviewed client source changed or is unavailable".to_string(),
            ));
        };
    }
    Ok((init, frontend.to_string()))
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn public_demo_owner_is_a_token_not_a_required_balance_amount() {
        let url=Url::parse("https://showcase.evo-games.com/frontend/evo/r2/#balance_id=demo-fixture&table_id=table123").unwrap();
        assert_eq!(
            demo_fragment(&url),
            Some(("demo-fixture".into(), "table123".into()))
        );
        let duplicate=Url::parse("https://showcase.evo-games.com/frontend/evo/r2/#balance_id=demo-a&balance_id=demo-b&table_id=table123").unwrap();
        assert!(demo_fragment(&duplicate).is_none());
    }
}
