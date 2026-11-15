//! Native protocol reconstructed from standalone info/irondog/codigo; no old runtime dependency.
use crate::model::{decimal, exact_mul, unknown_event};
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, now_ms},
};
use aes::{
    Aes256,
    cipher::{Block, BlockDecrypt, BlockEncrypt, KeyInit},
};
use anyhow::{Context, Result, bail, ensure};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::LazyLock, time::Duration};
use url::Url;
use uuid::Uuid;

const LAUNCHER: &str = "https://gamelauncherservice-stage.1x2networkhub.com/";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36";
fn selection(kind: &ModeKind, key: &str) -> Option<MorphicMode> {
    match kind {
        ModeKind::Normal if key == "normal" => Some(MorphicMode::Normal),
        ModeKind::Purchase => key
            .strip_prefix("buy ")
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .map(|id| MorphicMode::Purchase(id.into())),
        ModeKind::AnteBet => key
            .strip_prefix("ante ")
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .map(|id| MorphicMode::Ante(id.into())),
        _ => None,
    }
}

static INTEGRATED: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../bindings/irondog-integrated.json"))
        .expect("embedded Iron Dog IntegratedSlots contracts")
});
fn integrated_binding(game: &Game, mode: &Mode) -> bool {
    INTEGRATED["games"][&game.id]["modes"][&mode.key].as_str() == Some(mode.kind.as_str())
}
pub fn supports(game: &Game, mode: &Mode) -> bool {
    (matches!(game.id.as_str(), "90110" | "90140") && selection(&mode.kind, &mode.key).is_some())
        || integrated_binding(game, mode)
}

fn correlation(text: &str, seed: u32) -> String {
    let mut a = 3735928559 ^ seed;
    let mut b = 1103547991 ^ seed;
    for unit in text.encode_utf16() {
        a = (a ^ u32::from(unit)).wrapping_mul(2654435761);
        b = (b ^ u32::from(unit)).wrapping_mul(1597334677);
    }
    a = (a ^ (a >> 16)).wrapping_mul(2246822507) ^ (b ^ (b >> 13)).wrapping_mul(3266489909);
    b = (b ^ (b >> 16)).wrapping_mul(2246822507) ^ (a ^ (a >> 13)).wrapping_mul(3266489909);
    format!("{a:08x}{b:08x}")
}

fn encrypt(key: &str, text: &[u8]) -> Result<String> {
    let aes = Aes256::new_from_slice(key.as_bytes())
        .map_err(|_| anyhow::anyhow!("invalid Iron Dog gateway key length"))?;
    let mut previous = *Uuid::new_v4().as_bytes();
    let mut encoded = previous.to_vec();
    let mut data = text.to_vec();
    let padding = 16 - data.len() % 16;
    data.resize(data.len() + padding, padding as u8);
    for chunk in data.as_chunks::<16>().0 {
        let mut block = Block::<Aes256>::default();
        for (i, byte) in chunk.iter().enumerate() {
            block[i] = byte ^ previous[i];
        }
        aes.encrypt_block(&mut block);
        previous.copy_from_slice(&block);
        encoded.extend_from_slice(&block);
    }
    Ok(encoded.iter().map(|b| format!("{b:02x}")).collect())
}

fn decrypt(key: &str, text: &str) -> Result<Value> {
    ensure!(
        text.len() >= 64
            && text.len().is_multiple_of(32)
            && text.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid Iron Dog encrypted response"
    );
    let bytes = (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let aes = Aes256::new_from_slice(key.as_bytes())
        .map_err(|_| anyhow::anyhow!("invalid Iron Dog gateway key length"))?;
    let mut plaintext = Vec::new();
    for (previous, current) in bytes
        .as_chunks::<16>()
        .0
        .iter()
        .zip(bytes.as_chunks::<16>().0.iter().skip(1))
    {
        let mut block = Block::<Aes256>::clone_from_slice(current);
        aes.decrypt_block(&mut block);
        plaintext.extend(block.iter().zip(previous).map(|(a, b)| a ^ b));
    }
    let padding = usize::from(
        *plaintext
            .last()
            .context("empty Iron Dog gateway response")?,
    );
    ensure!(
        (1..=16).contains(&padding)
            && plaintext[plaintext.len() - padding..]
                .iter()
                .all(|&b| usize::from(b) == padding),
        "invalid Iron Dog gateway padding"
    );
    plaintext.truncate(plaintext.len() - padding);
    Ok(serde_json::from_slice(&plaintext)?)
}

fn text<'a>(v: &'a Value, name: &str) -> Result<&'a str> {
    v[name]
        .as_str()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("missing Iron Dog field {name}"))
}

struct Gateway<'a> {
    ctx: &'a RunContext,
    url: Url,
    headers: BTreeMap<String, String>,
    token: String,
}

impl Gateway<'_> {
    async fn exchange(
        &mut self,
        action: &str,
        target: &str,
        payload: &Value,
        round_action: bool,
    ) -> Result<Value> {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let mut url = self.url.clone();
        url.query_pairs_mut().append_pair("target", target);
        self.ctx.note(
            "native_request",
            &json!({"action":action,"request":payload}),
        )?;
        let encrypted = encrypt(&self.token, &serde_json::to_vec(payload)?)?;
        let response = self
            .ctx
            .json(
                action,
                url.as_str(),
                &json!({"data":encrypted}),
                self.headers.clone(),
                round_action,
            )
            .await?;
        let outer = decrypt(&self.token, text(&response["response"], "data")?).map_err(|e| {
            unknown_event(format!("Iron Dog native encrypted envelope changed: {e}"))
        })?;
        self.token = text(&outer, "token")?.to_owned();
        let native: Value = serde_json::from_str(text(&outer, "data")?)?;
        self.ctx.note(
            "native_response",
            &json!({"action":action,"response":native}),
        )?;
        Ok(native)
    }
    async fn command(
        &mut self,
        action: &str,
        payload: &Value,
        round_action: bool,
    ) -> Result<Value> {
        let native = self
            .exchange(
                action,
                &format!("http://localhost:81/integratedslotsduck/services/v1/duck/{action}"),
                payload,
                round_action,
            )
            .await?;
        ensure!(
            native["success"] == true,
            "Iron Dog Morphic native command rejected"
        );
        Ok(native["payload"].clone())
    }
    async fn integrated_command(
        &mut self,
        action: &str,
        payload: &Value,
        round_action: bool,
    ) -> Result<Value> {
        let native = self
            .exchange(
                action,
                "http://localhost:81/IntegratedSlots/game",
                payload,
                round_action,
            )
            .await?;
        let response = native
            .get("response")
            .filter(|v| v.is_object())
            .ok_or_else(|| unknown_event("Iron Dog IntegratedSlots response envelope changed"))?;
        if let Some(code) = response.get("errorcode").filter(|v| !v.is_null()) {
            let value = code
                .as_i64()
                .or_else(|| code.as_str().and_then(|s| s.parse::<i64>().ok()))
                .ok_or_else(|| {
                    unknown_event("Iron Dog IntegratedSlots error code shape changed")
                })?;
            ensure!(
                value == 0,
                "Iron Dog IntegratedSlots native rejection {value}"
            );
        }
        Ok(response.clone())
    }
}

struct IntegratedQuote {
    stake: Value,
    modifiers: Value,
    repeat: bool,
}
fn native_number(n: Decimal) -> Result<Value> {
    Ok(serde_json::from_str(&n.normalize().to_string())?)
}
fn quote_integrated(setup: &Value, mode: &MorphicMode) -> Result<IntegratedQuote> {
    let cfg = &setup["response"]["config"];
    if !matches!(
        cfg["type"].as_str(),
        Some("PAYLINES" | "WINWAYS" | "CLUSTERPAYS")
    ) {
        return Err(unknown_event(
            "Iron Dog IntegratedSlots config family changed",
        ));
    }
    let values = cfg["betValues"]
        .as_array()
        .ok_or_else(|| unknown_event("Iron Dog native stake menu shape changed"))?;
    let stake = values
        .iter()
        .map(amount)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|v| *v > Decimal::ZERO)
        .min()
        .context("Iron Dog native stake menu empty")?;
    let stake = exact_mul(stake, Decimal::from(100))?;
    ensure!(
        stake.fract() == Decimal::ZERO,
        "Iron Dog native stake cannot be encoded in cents"
    );
    let mut modifiers = json!({});
    let mut repeat = false;
    match mode {
        MorphicMode::Normal => {}
        MorphicMode::Ante(id) => {
            ensure!(cfg["disableAnteBet"] != true, "Iron Dog ante unavailable");
            let option = &cfg["anteBetSetup"]["options"][id];
            let scale = amount(&option["stakeScale"])?;
            ensure!(scale > Decimal::ZERO, "Iron Dog native ante unavailable");
            modifiers["isAnte"] = json!(true);
            // The native contract encodes this per-option value with two decimal places.
            modifiers[format!("anteBetSelection_{id}")] =
                native_number(exact_mul(stake, scale)?.round_dp(2))?;
            repeat = true;
        }
        MorphicMode::Purchase(id) => {
            ensure!(cfg["disableBuyIn"] != true, "Iron Dog buy unavailable");
            let groups = cfg["buyInSetup"]["options"]
                .as_array()
                .context("Iron Dog buy menu absent")?;
            if groups.len() != 1 {
                return Err(unknown_event("Iron Dog buy menu group structure changed"));
            }
            let prices = groups[0]["price"]
                .as_array()
                .context("Iron Dog buy option list absent")?;
            let mut matches = prices
                .iter()
                .filter(|p| identity(&p["optionString"]).ok().as_ref() == Some(id));
            let option = matches
                .next()
                .context("Iron Dog requested buy option absent")?;
            if matches.next().is_some() {
                return Err(unknown_event("Iron Dog duplicate buy option"));
            }
            let scale = amount(&option["stakeScale"])?;
            ensure!(scale > Decimal::ONE, "Iron Dog buy scale unavailable");
            if let Some(limit) = option.get("maxBet").filter(|v| !v.is_null()) {
                let limit = amount(limit)?;
                ensure!(
                    limit == Decimal::ZERO || stake <= limit,
                    "Iron Dog buy exceeds native option bound"
                );
            }
            let lines = if let Some(lines) = cfg["lines"].as_array() {
                u64::try_from(lines.len())?
            } else {
                integer(&cfg["coinsPlayed"])?
            };
            ensure!(lines > 0, "Iron Dog native line count absent");
            modifiers = json!({"buyIn":"true","buyInValue":native_number(exact_mul(stake,scale)?.round_dp(2))?,
                "selection_0":option["optionString"],"lines":lines.to_string(),"conf":"2","mobile":"false"});
            if let Some(spins) = option["spins"].as_u64() {
                modifiers["nSpins"] = json!(spins.to_string());
            }
        }
    }
    Ok(IntegratedQuote {
        stake: native_number(stake)?,
        modifiers,
        repeat,
    })
}
fn integrated_spin(
    common: &Value,
    guest: u64,
    record: Option<&Value>,
    quote: &IntegratedQuote,
    first: bool,
) -> Result<Value> {
    let mut payload = common.clone();
    let fields = payload
        .as_object_mut()
        .context("Iron Dog common request must be an object")?;
    fields.extend(json!({"mode":"spin","isPromoRound":0,"promoActivationID":0,"promoCampaignID":0,"promoType":0,
        "stake":quote.stake,"now":now_ms(),"settings":"011","accountID":guest,"customerID":"guest"}).as_object().unwrap().clone());
    if let Some(record) =
        record.filter(|v| !v.is_null() && v.as_u64() != Some(0) && v.as_str() != Some(""))
    {
        let record = identity(record)?;
        fields.insert("accountID".into(), json!(format!("{guest}||{record}")));
        fields.insert("customerID".into(), json!(format!("guest||{record}")));
    }
    if first || quote.repeat {
        fields.extend(
            quote
                .modifiers
                .as_object()
                .context("Iron Dog native modifiers must be an object")?
                .clone(),
        );
    }
    Ok(payload)
}
fn integrated_settled(response: &Value) -> Result<bool> {
    identity(&response["bet"]["id"])
        .map_err(|_| unknown_event("Iron Dog IntegratedSlots bet identity missing"))?;
    response["bet"]["settle"]
        .as_bool()
        .ok_or_else(|| unknown_event("Iron Dog IntegratedSlots settlement marker missing"))
}

fn integrated_headers(origin: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("origin".into(), origin.into()),
        ("user-agent".into(), UA.into()),
        ("accept".into(), "*/*".into()),
        ("accept-language".into(), "en-US,en;q=0.9".into()),
    ])
}

async fn execute_integrated(ctx: &RunContext, game: &Game, mode: &Mode) -> Result<Completion> {
    let gid = game.id.parse::<u64>()?;
    let profile = [
        ("siteID", "809".to_owned()),
        ("clientID", "184".to_owned()),
        ("sessionID", "demo".to_owned()),
        ("gameID", game.id.clone()),
        ("playMode", "fun".to_owned()),
        ("lang", "en".to_owned()),
        ("platform", "mobile".to_owned()),
        ("jurisdiction", "com".to_owned()),
    ];
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(&profile)
        .finish();
    let seed = format!("{query}|{}|{}", now_ms(), Uuid::new_v4());
    let hash = format!("{}{}", correlation(&seed, 0), correlation(&seed, 1));
    let mut params: Vec<_> = profile
        .iter()
        .map(|(key, value)| json!({"key":key,"value":value}))
        .collect();
    params.push(json!({"key":"userAgent","value":UA}));
    let launch = ctx
        .json(
            "public_fun_launcher",
            "https://gamelauncherservice.desertorchidhub.com/",
            &json!({"clientID":"184","hash":hash,"payload":params,"userAgent":UA}),
            integrated_headers("https://gamelauncher.desertorchidhub.com"),
            false,
        )
        .await?;
    let binding = validate_launch(&launch, gid)?;
    let p = &launch["data"]["parameters"];
    if launch["success"] != true
        || p["playMode"] != "fun"
        || binding.morphic
        || binding.gateway != "https://gp.sociumhubeurope.com/gateway/"
        || !matches!(p["gameType"].as_str(), Some("PREMIUMSLOTS" | "OPENSLOTS"))
    {
        return Err(unknown_event(
            "Iron Dog IntegratedSlots launcher contract changed",
        ));
    }
    ensure!(
        integer(&p["site"])? == 809,
        "Iron Dog public demo site changed"
    );
    let mut headers = integrated_headers("https://cdn.desertorchidhub.com");
    headers.insert("referer".into(), "https://cdn.desertorchidhub.com/".into());
    let guest = ctx
        .http(
            "guest_id",
            "GET",
            &binding.guest_path,
            headers.clone(),
            String::new(),
            false,
        )
        .await?;
    ensure!(
        guest.status == 200,
        "Iron Dog guest endpoint returned HTTP {}",
        guest.status
    );
    let mut guests =
        url::form_urlencoded::parse(guest.body.as_bytes()).filter(|(k, _)| k == "guestID");
    let guest = guests
        .next()
        .context("Iron Dog guest identity absent")?
        .1
        .parse::<u64>()?;
    if guest == 0 || guests.next().is_some() {
        return Err(unknown_event("Iron Dog guest identity ambiguous"));
    }
    headers.insert("x-do-not-track".into(), "c2ad63fd79f2".into());
    headers.insert("x-csrf-token".into(), game.id.clone());
    let url = Url::parse(&binding.gateway)?;
    let init = ctx
        .http(
            "gateway_init",
            "POST",
            url.join("init")?.as_str(),
            headers.clone(),
            String::new(),
            false,
        )
        .await?;
    ensure!(
        init.status == 201,
        "Iron Dog gateway init returned HTTP {}",
        init.status
    );
    let init: Value = serde_json::from_str(&init.body)?;
    headers.insert("x-requested-with".into(), text(&init, "sid")?.into());
    headers.insert(
        "x-requested-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    let mut gateway = Gateway {
        ctx,
        url,
        headers,
        token: text(&init, "token")?.into(),
    };
    identity(&p["gameVersion"])?;
    let common = json!({"gameID":gid,"username":"guest","customerID":"guest","fun_real":-1,"ex_rate":1,"currency":"FUN",
        "site":809,"version":p["gameVersion"],"jurisdiction":p["jurisdiction"].as_str().unwrap_or(""),"gameType":p["gameType"]});
    let mut request = common.clone();
    request["mode"] = json!("setup");
    request["accountID"] = json!(guest);
    request["stake"] = json!("100");
    request["now"] = json!(now_ms());
    let setup = gateway.integrated_command("setup", &request, false).await?;
    let quote = quote_integrated(
        &json!({"response":setup}),
        &selection(&mode.kind, &mode.key).context("Iron Dog native selector absent")?,
    )?;
    ctx.save_session(&json!({"provider":"irondog","engine":"IntegratedSlots","game":gid,"guest":guest,"sid":init["sid"],"token":gateway.token,"gateway":gateway.url.as_str()}))?;
    let mut record = setup["bet"].get("betRecord").cloned();
    let mut first_id = None;
    for step in 0..ctx.max_steps() {
        let payload = integrated_spin(&common, guest, record.as_ref(), &quote, step == 0)?;
        let result = gateway.integrated_command("spin", &payload, true).await?;
        let closed = integrated_settled(&result)?;
        if first_id.is_none() {
            first_id = Some(identity(&result["bet"]["id"])?);
        }
        if closed {
            return Ok(Completion {
                native_round_id: first_id,
                details: json!({"game":gid,"engine":"IntegratedSlots","steps":step+1}),
            });
        }
        record = result["bet"].get("betRecord").cloned();
    }
    bail!("Iron Dog IntegratedSlots continuation limit reached")
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    ensure!(
        supports(game, mode),
        "Iron Dog reviewed game or mode unavailable"
    );
    if integrated_binding(game, mode) {
        return execute_integrated(ctx, game, mode).await;
    }
    let gid = game.id.parse::<u64>()?;
    let demo_id = match gid {
        90110 => "182733",
        90140 => "182743",
        _ => bail!("Iron Dog public demo link unavailable"),
    };
    let entry = ctx.get("public_demo_link", &format!("https://nogs-gl-stage.nyxmalta.com/game/?clienttype=flash&nogscurrency=EUR&nogslang=es_ES&nogsmode=demo&nogsoperatorid=961&nogsgameid={demo_id}")).await?;
    ensure!(
        matches!(entry.status, 301 | 302 | 303 | 307 | 308),
        "Iron Dog public demo link did not redirect"
    );
    let redirect = Url::parse(
        entry
            .headers
            .get("location")
            .context("missing Iron Dog redirect")?,
    )?;
    ensure!(
        redirect.origin().ascii_serialization() == "https://ogs-gcm-eu-stage.nyxop.net"
            && redirect.path() == "/gcm/gcm-launcher/launcher.html",
        "unexpected Iron Dog public demo redirect"
    );
    let mut profile: BTreeMap<String, String> = redirect.query_pairs().into_owned().collect();
    ensure!(
        profile.get("mode").is_some_and(|mode| mode == "demo")
            && profile
                .get("sessionid")
                .is_some_and(|sid| sid.starts_with("Free:"))
            && profile.get("gameid") == Some(&game.id),
        "Iron Dog redirect did not confirm this demo"
    );
    let base = profile
        .remove("gameUrl")
        .context("missing Iron Dog launcher base")?;
    let base_url = Url::parse(&base)?;
    ensure!(
        base_url.origin().ascii_serialization() == "https://gamelauncher-stage.1x2networkhub.com"
            && base_url.path() == "/playGame.html",
        "unexpected Iron Dog launcher base"
    );
    let base_fields: BTreeMap<String, String> = base_url.query_pairs().into_owned().collect();
    let client_id = base_fields
        .get("clientID")
        .context("missing Iron Dog public client ID")?
        .parse::<u64>()?;
    let mut query_fields = base_fields.clone();
    query_fields.extend(profile.clone());
    query_fields.insert("realwinloss".into(), "n".into());
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(&query_fields)
        .finish();
    let seed = format!("{query}|{}|{}", now_ms(), Uuid::new_v4());
    let hash = format!("{}{}", correlation(&seed, 0), correlation(&seed, 1));
    let device = profile
        .get("device")
        .context("missing Iron Dog demo device")?
        .clone();
    let query_encoded = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    profile.extend(base_fields);
    profile.extend([
        ("gameUrl".into(), base),
        ("playmode".into(), "demo".into()),
        ("loggedin".into(), "true".into()),
        ("gameName".into(), game.id.clone()),
        ("NYX_GCM_ENV".into(), "stage".into()),
        ("NYX_GCM_DEVICE".into(), device),
        (
            "NYX_GCM_PARAMS".into(),
            format!("/playGame.html?{query_encoded}"),
        ),
        ("userAgent".into(), UA.into()),
    ]);
    let params: Vec<_> = profile
        .into_iter()
        .map(|(key, value)| json!({"key":key,"value":value}))
        .collect();
    let launch = ctx
        .json(
            "public_fun_launcher",
            LAUNCHER,
            &json!({"clientID":client_id,"hash":hash,"payload":params,"userAgent":UA}),
            BTreeMap::from([(
                "origin".into(),
                "https://gamelauncher-stage.1x2networkhub.com".into(),
            )]),
            false,
        )
        .await?;
    let binding = validate_launch(&launch, gid)?;
    ensure!(binding.morphic, "Iron Dog game requires a different engine");
    let mut p = launch["data"]["parameters"].clone();
    if let Some(extras) = launch["data"]["extras"].as_array() {
        for extra in extras {
            let key = text(extra, "key")?;
            if matches!(
                key,
                "sessionID"
                    | "operatorID"
                    | "operatorBrandID"
                    | "installID"
                    | "accountID"
                    | "extInstallID"
                    | "jurisdiction"
            ) {
                p[key] = extra["value"].clone();
            }
        }
    }
    let url = Url::parse(&binding.gateway)?;
    let origin = if url.host_str() == Some("gp-stage.1x2hubeurope.com") {
        "https://cdn-stage.1x2networkhub.com"
    } else {
        "https://cdn.desertorchidhub.com"
    };
    let mut headers = BTreeMap::from([
        ("origin".into(), origin.into()),
        ("referer".into(), format!("{origin}/")),
        ("user-agent".into(), UA.into()),
        ("accept-language".into(), "es-ES,es;q=0.9,en;q=0.8".into()),
        ("x-do-not-track".into(), "9d9f33c4d440".into()),
        ("x-csrf-token".into(), game.id.clone()),
    ]);
    let init = ctx
        .http(
            "gateway_init",
            "POST",
            url.join("init")?.as_str(),
            {
                let mut init_headers = headers.clone();
                init_headers.insert("content-length".into(), "0".into());
                init_headers
            },
            String::new(),
            false,
        )
        .await?;
    ensure!(
        init.status == 201,
        "Iron Dog gateway init returned HTTP {}",
        init.status
    );
    let native: Value = serde_json::from_str(&init.body)?;
    headers.insert("x-requested-with".into(), text(&native, "sid")?.into());
    headers.insert("x-requested-type".into(), "application/json".into());
    let mut gateway = Gateway {
        ctx,
        url,
        headers,
        token: text(&native, "token")?.into(),
    };
    let mut setup_req = json!({"realMode":false,"sessionID":text(&p,"sessionID")?,"operatorID":integer(&p["operatorID"])?,
        "operatorBrandID":integer(&p["operatorBrandID"])?,"installID":integer(&p["installID"])?,
        "accountID":p["accountID"],"gameID":gid,"jurisdiction":p["jurisdiction"].as_str().filter(|s| !s.is_empty()).unwrap_or("com")});
    if !p["extInstallID"].is_null() {
        setup_req["extInstallID"] = p["extInstallID"].clone();
    }
    let setup = gateway.command("setup", &setup_req, false).await?;
    let quote = quote_morphic(
        &setup,
        gid,
        &selection(&mode.kind, &mode.key).context("Iron Dog selector missing")?,
    )?;
    let account = &setup["account"];
    identity(&account["accountID"])?;
    identity(&account["engineSessionID"])?;
    ctx.save_session(&json!({"provider":"irondog","game":gid,"account":account,"gateway":gateway.url.as_str(),"sid":native["sid"],"token":gateway.token,"currency":"FUN"}))?;
    let common = json!({"realMode":false,"gameID":gid,"operatorID":setup_req["operatorID"],"operatorBrandID":setup_req["operatorBrandID"],
        "jurisdiction":setup_req["jurisdiction"],"accountID":account["accountID"],"engineSessionID":account["engineSessionID"],"stake":quote.stake_unit});
    let mut steps = Vec::new();
    for index in 0..ctx.max_steps().min(500) {
        let mut payload = common.clone();
        if (index == 0 || quote.repeat_modifiers)
            && let Some(modifiers) = quote.modifiers.as_object()
        {
            payload
                .as_object_mut()
                .context("invalid Iron Dog spin payload")?
                .extend(modifiers.clone());
        }
        let step = gateway.command("spin", &payload, true).await?;
        let done = step["bet"]["settled"]
            .as_bool()
            .ok_or_else(|| unknown_event("Iron Dog settlement marker missing"))?;
        steps.push(step);
        if done {
            return validate_round(&setup, &steps, gid);
        }
        if !(steps
            .last()
            .and_then(|s| s.pointer("/state/phase"))
            .and_then(Value::as_array)
            .is_some_and(|a| {
                !a.is_empty()
                    && a.iter()
                        .all(|p| matches!(p.as_str(), Some("BASE" | "HOLD_AND_WIN")))
            }))
        {
            return Err(unknown_event(
                "Iron Dog native phase requires a different control",
            ));
        }
    }
    bail!("Iron Dog continuation limit reached")
}

fn amount(value: &Value) -> Result<Decimal> {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => bail!("Iron Dog native amount missing"),
    };
    decimal(&text)
}

fn integer(value: &Value) -> Result<u64> {
    match value {
        Value::Number(number) => number.as_u64().context("Iron Dog integer required"),
        Value::String(text) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => {
            text.parse().context("Iron Dog integer outside range")
        }
        _ => bail!("Iron Dog integer required"),
    }
}

fn identity(value: &Value) -> Result<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Ok(text.clone()),
        Value::Number(number) if number.as_u64().is_some() => Ok(number.to_string()),
        _ => bail!("Iron Dog native round identity missing"),
    }
}

#[derive(Debug, Serialize)]
pub struct GatewayBinding {
    pub game_id: u64,
    pub gateway: String,
    pub guest_path: String,
    pub morphic: bool,
}

pub fn validate_launch(launch: &Value, expected_game: u64) -> Result<GatewayBinding> {
    let parameters = &launch["data"]["parameters"];
    ensure!(
        integer(&parameters["gameID"])? == expected_game,
        "Iron Dog launch game changed"
    );
    ensure!(
        matches!(parameters["playMode"].as_str(), Some("fun" | "demo")),
        "Iron Dog fresh public fun launch required"
    );
    let path = Url::parse(
        parameters["path"]
            .as_str()
            .context("Iron Dog launch path missing")?,
    )?;
    ensure!(
        path.scheme() == "https"
            && matches!(
                path.host_str(),
                Some("gp.sociumhubeurope.com" | "gp-stage.1x2hubeurope.com")
            )
            && path.path() == "/f1x2games/"
            && path.query().is_none()
            && path.fragment().is_none()
            && path.username().is_empty()
            && path.password().is_none()
            && path.port().is_none_or(|p| p == 443),
        "Iron Dog native demo gateway is not the documented authority"
    );
    Ok(GatewayBinding {
        game_id: expected_game,
        gateway: path.join("/gateway/")?.into(),
        guest_path: path.join("getGuestID.jsp")?.into(),
        morphic: launch["data"]["gamePath"]
            .as_str()
            .is_some_and(|p| p.starts_with("games/")),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum MorphicMode {
    Normal,
    Purchase(String),
    Ante(String),
}

#[derive(Debug, Serialize)]
pub struct MorphicQuote {
    pub game_id: u64,
    pub stake_unit: Value,
    pub modifiers: Value,
    pub repeat_modifiers: bool,
}

/// Uses only current native offers. Cost in currency is the chosen denomination
/// times the native multiplier, avoiding rounded division of unit values.
pub fn quote_morphic(setup: &Value, game: u64, mode: &MorphicMode) -> Result<MorphicQuote> {
    ensure!(
        setup["account"]["real"] == false && integer(&setup["account"]["gameID"])? == game,
        "Iron Dog Morphic setup is not the requested FUN game"
    );
    ensure!(
        setup["bet"]["settled"] != false,
        "Iron Dog setup has an open native round"
    );
    let values = setup["config"]["betValues"]
        .as_array()
        .context("Iron Dog betValues missing")?;
    let mut minimum: Option<(Decimal, &Value)> = None;
    for value in values {
        let unit = amount(&value["unit"])?;
        ensure!(unit > Decimal::ZERO, "Iron Dog zero native denomination");
        if minimum.as_ref().is_none_or(|previous| unit < previous.0) {
            minimum = Some((unit, value));
        }
    }
    let (unit, value) = minimum.context("Iron Dog native denomination list empty")?;
    let mut modifiers = json!({});
    let mut repeat = false;
    match mode {
        MorphicMode::Normal => {}
        MorphicMode::Purchase(id) => {
            let option = &setup["config"]["betFeatures"]["buyIn"]["options"][id];
            ensure!(
                option["enabled"] == true,
                "Iron Dog purchase is not enabled"
            );
            let multiplier = amount(&option["timesStake"])?;
            ensure!(
                multiplier > Decimal::ZERO,
                "Iron Dog purchase multiplier must be positive"
            );
            let cost = exact_mul(unit, multiplier)?;
            ensure!(
                cost >= amount(&option["minStake"]["unit"])?
                    && cost <= amount(&option["maxStake"]["unit"])?,
                "Iron Dog purchase outside native bounds"
            );
            modifiers =
                json!({"buyInId":id,"buyInCost":serde_json::from_str::<Value>(&cost.to_string())?});
        }
        MorphicMode::Ante(id) => {
            ensure!(
                !id.is_empty() && !id.contains(','),
                "Iron Dog only a single native ante is documented"
            );
            let option = &setup["config"]["anteBets"][id];
            let multiplier = amount(&option["multiplier"])?;
            ensure!(
                multiplier > Decimal::ZERO,
                "Iron Dog ante multiplier must be positive"
            );
            modifiers = json!({"anteBets":[id],"totalStake":
                serde_json::from_str::<Value>(&exact_mul(unit,multiplier)?.to_string())?});
            repeat = true;
        }
    }
    Ok(MorphicQuote {
        game_id: game,
        stake_unit: value["unit"].clone(),
        modifiers,
        repeat_modifiers: repeat,
    })
}

fn validate_round(setup: &Value, steps: &[Value], game: u64) -> Result<Completion> {
    let mut root = None;
    for (index, step) in steps.iter().enumerate() {
        let bet = &step["bet"];
        let id = identity(bet.get("betRecordID").unwrap_or(&bet["id"]))?;
        ensure!(
            root.as_ref().is_none_or(|old| old == &id),
            "Iron Dog native round changed"
        );
        root = Some(id);
        if bet["settled"].as_bool() != Some(index + 1 == steps.len()) {
            return Err(unknown_event("Iron Dog native settled marker not terminal"));
        }
        for key in ["accountID", "engineSessionID"] {
            if let Some(actual) = step["account"].get(key) {
                ensure!(
                    actual == &setup["account"][key],
                    "Iron Dog account/session owner changed"
                );
            }
        }
        if let Some(gid) = step["account"].get("gameID") {
            ensure!(integer(gid)? == game, "Iron Dog game changed");
        }
    }
    Ok(Completion {
        native_round_id: Some(root.context("Iron Dog native round missing")?),
        details: json!({"game":game,"steps":steps.len()}),
    })
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn native_terminal_round_without_balance_or_wins() {
        let setup = json!({"account":{"gameID":90110,"accountID":"a","engineSessionID":"s"}});
        let done = json!({"bet":{"id":"r","settled":true},"account":{"gameID":90110,"accountID":"a","engineSessionID":"s"}});
        assert_eq!(
            validate_round(&setup, &[done], 90110)
                .unwrap()
                .native_round_id
                .as_deref(),
            Some("r")
        );
    }
    #[test]
    fn open_native_marker_cannot_be_completion() {
        assert!(
            validate_round(
                &json!({}),
                &[json!({"bet":{"id":"r","settled":false}})],
                90110
            )
            .unwrap_err()
            .downcast_ref::<crate::model::UnknownEvent>()
            .is_some()
        );
    }
    #[test]
    fn normal_quote_needs_native_units_without_currency() {
        let setup =
            json!({"account":{"real":false,"gameID":90110},"config":{"betValues":[{"unit":10}]}});
        assert_eq!(
            quote_morphic(&setup, 90110, &MorphicMode::Normal)
                .unwrap()
                .stake_unit,
            10
        );
    }
}

#[cfg(test)]
mod recovered_tests {
    use super::*;

    #[test]
    fn integrated_launcher_guest_and_gateway_keep_required_client_language() {
        let launcher = integrated_headers("https://gamelauncher.desertorchidhub.com");
        let guest = integrated_headers("https://cdn.desertorchidhub.com");
        let mut gateway = guest.clone();
        gateway.insert("x-csrf-token".into(), "8150".into());
        gateway.insert("x-requested-with".into(), "fresh-gateway-sid".into());
        gateway.insert(
            "x-requested-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        // Preserve the native harvester's exact language header on every HTTP request.
        for headers in [launcher, guest, gateway] {
            assert_eq!(
                headers.get("accept-language").map(String::as_str),
                Some("en-US,en;q=0.9")
            );
            assert_eq!(headers.get("accept").map(String::as_str), Some("*/*"));
        }
    }
    fn setup() -> Value {
        json!({"response":{"config":{"type":"PAYLINES","betValues":[0.2,0.5],"lines":[[],[],[],[],[]],"anteBetSetup":{"options":{"100":{"stakeScale":1.5}}},"buyInSetup":{"options":[{"price":[{"optionString":0,"stakeScale":50,"spins":10}]}]}}}})
    }
    #[test]
    fn integrated_native_units_and_modifiers_need_no_balance() {
        let q = quote_integrated(&setup(), &MorphicMode::Purchase("0".into())).unwrap();
        assert_eq!(q.stake, json!(20));
        assert_eq!(
            q.modifiers,
            json!({"buyIn":"true","buyInValue":1000,"selection_0":0,"nSpins":"10","lines":"5","conf":"2","mobile":"false"})
        );
        assert!(!q.repeat);
        let a = quote_integrated(&setup(), &MorphicMode::Ante("100".into())).unwrap();
        assert_eq!(
            a.modifiers,
            json!({"isAnte":true,"anteBetSelection_100":30})
        );
        assert!(a.repeat);
        assert!(quote_integrated(&setup(), &MorphicMode::Purchase("99".into())).is_err());
    }
    #[test]
    fn integrated_record_is_carried_and_buy_is_only_on_entry() {
        let q = quote_integrated(&setup(), &MorphicMode::Purchase("0".into())).unwrap();
        let common = json!({"gameID":8150,"customerID":"guest"});
        let first = integrated_spin(&common, 123, None, &q, true).unwrap();
        let next = integrated_spin(&common, 123, Some(&json!(77)), &q, false).unwrap();
        assert_eq!(first["accountID"], 123);
        assert_eq!(first["selection_0"], 0);
        assert_eq!(next["accountID"], "123||77");
        assert_eq!(next["customerID"], "guest||77");
        assert!(next.get("buyIn").is_none());
    }
    #[test]
    fn integrated_terminal_marker_is_native_and_unknown_shape_alerts() {
        assert!(integrated_settled(&json!({"bet":{"id":9,"settle":true,"balance":null}})).unwrap());
        assert!(!integrated_settled(&json!({"bet":{"id":9,"settle":false}})).unwrap());
        assert!(
            integrated_settled(&json!({"bet":{"id":9,"newState":"pick"}}))
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
