//! Native FREE capture protocols, including the newly delivered legacy harvest contracts.
//! Sources and their hashes are pinned in bindings/betsoft-extra.json.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind, decimal, now_ms, unknown_event},
};
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::LazyLock, time::Duration};
use url::{Url, form_urlencoded};

type Form = BTreeMap<String, String>;
static BINDINGS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../bindings/betsoft-extra.json"))
        .expect("embedded Betsoft legacy contracts")
});
static NATIVE_TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"TIME=\d+").expect("Betsoft native timestamp pattern"));

fn form(text: &str) -> Result<Form> {
    let mut fields = Form::new();
    for (key, value) in form_urlencoded::parse(text.as_bytes()) {
        if fields
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(unknown_event("duplicate Betsoft form field"));
        }
    }
    Ok(fields)
}
fn field<'a>(fields: &'a Form, key: &str) -> Result<&'a str> {
    fields
        .get(key)
        .map(String::as_str)
        .with_context(|| format!("Betsoft field {key} missing"))
}
fn reviewed(gid: &str, list: &str) -> bool {
    gid.parse::<u64>().ok().is_some_and(|id| {
        BINDINGS[list]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|v| v.as_u64() == Some(id)))
    })
}
pub fn supports(game: &Game, mode: &Mode) -> bool {
    BINDINGS["games"][&game.id]["modes"][&mode.key].as_str() == Some(mode.kind.as_str())
}

#[derive(Debug)]
enum Choice {
    Normal,
    Buy(usize),
    Boost(usize),
}
fn choice(mode: &Mode) -> Result<Choice> {
    match mode.kind {
        ModeKind::Normal if mode.key == "normal" => Ok(Choice::Normal),
        ModeKind::Purchase => Ok(Choice::Buy(mode.key.parse()?)),
        ModeKind::Booster if mode.key == "boost" => Ok(Choice::Boost(0)),
        ModeKind::Booster if mode.key.starts_with("boost-") => {
            Ok(Choice::Boost(mode.key[6..].parse()?))
        }
        _ => bail!("Betsoft selector has no native command contract"),
    }
}
fn positive_number(v: &str) -> Result<Decimal> {
    let v = decimal(v)?;
    ensure!(v > Decimal::ZERO, "Betsoft native option must be positive");
    Ok(v)
}
fn positive_index(v: &str) -> Result<u64> {
    let n = v.parse::<u64>()?;
    ensure!(n > 0, "Betsoft native line count must be positive");
    Ok(n)
}

fn coin_index(init: &Form, mode: &Mode) -> Result<usize> {
    let coins = field(init, "IBETVALUES")?
        .split_whitespace()
        .map(positive_number)
        .collect::<Result<Vec<_>>>()?;
    let allowed = if let (Choice::Buy(index), Some(groups)) =
        (choice(mode)?, init.get("BUY_FEATURE_ALLOWED_COINS"))
    {
        let groups: Vec<_> = groups.split('|').collect();
        let group = if groups.len() == 1 {
            groups[0]
        } else {
            *groups
                .get(index)
                .context("Betsoft requested buy coin group unavailable")?
        };
        Some(
            group
                .split_whitespace()
                .map(positive_number)
                .collect::<Result<Vec<_>>>()?,
        )
    } else {
        None
    };
    coins
        .iter()
        .enumerate()
        .filter(|(_, c)| allowed.as_ref().is_none_or(|a| a.contains(c)))
        .min_by_key(|(_, c)| **c)
        .map(|(i, _)| i)
        .context("Betsoft positive allowed coin offer missing")
}

fn purchase_price(gid: &str, cfg: &Form, index: usize) -> Result<()> {
    ensure!(
        reviewed(gid, "buy_reviewed"),
        "Betsoft purchase contract unavailable"
    );
    if matches!(gid, "794" | "796" | "798" | "801" | "800") {
        ensure!(index == 0, "Betsoft only the native purchase is available");
        let value = decimal(
            field(cfg, "BUY_PARAM")?
                .split('|')
                .next()
                .context("Betsoft buy parameter absent")?,
        )?;
        ensure!(value >= Decimal::ZERO, "Betsoft buy parameter invalid");
        return Ok(());
    }
    if gid == "934" {
        let counts: Vec<_> = field(cfg, "BUY_FS_COUNT")?.split('|').collect();
        ensure!(
            index <= counts.len(),
            "Betsoft requested free-spin purchase unavailable"
        );
        if index == counts.len() {
            positive_number(field(cfg, "BUY_RESPIN_PRICE")?)?;
            return Ok(());
        }
        positive_index(counts[index])?;
    }
    let combined = if gid == "973" {
        Some(format!(
            "{}|{}",
            field(cfg, "BUY_PRICE_RANDOM")?,
            field(cfg, "BUY_PRICE_ALL")?
        ))
    } else {
        None
    };
    let prices = combined
        .as_deref()
        .or_else(|| {
            ["BUYPRICE", "BUY_PRICE", "BUY_PRICES"]
                .into_iter()
                .find_map(|k| cfg.get(k).map(String::as_str))
        })
        .context("Betsoft native purchase menu unavailable")?;
    positive_number(
        prices
            .split('|')
            .nth(index)
            .context("Betsoft purchase index unavailable")?,
    )?;
    Ok(())
}

fn build_command(gid: &str, cfg: &Form, mode: &Mode, coin: usize) -> Result<Form> {
    let lines = positive_index(field(cfg, "DEFAULTNUMLINES")?)?;
    let selected = choice(mode)?;
    let mut command = Form::from([
        ("CMD".into(), "PLACEBET".into()),
        ("BET".into(), format!("{coin}|{lines}|1")),
        ("AUTOPLAYSPIN".into(), "FALSE".into()),
        (
            "BUY".into(),
            if matches!(selected, Choice::Buy(_)) {
                "TRUE"
            } else {
                "FALSE"
            }
            .into(),
        ),
    ]);
    if gid == "2" {
        return Ok(Form::from([
            ("CMD".into(), "PLACEBET".into()),
            ("BET".into(), format!("{coin} 1 1")),
        ]));
    }
    if gid == "426" {
        ensure!(
            matches!(selected, Choice::Normal),
            "Betsoft game type variant lacks a purchase contract"
        );
        let game_type = field(cfg, "DEF_GAME_TYPE")?;
        ensure!(!game_type.is_empty(), "Betsoft native game type absent");
        return Ok(Form::from([
            ("CMD".into(), "PLACEBET".into()),
            ("BET".into(), format!("{coin}|{lines}|1")),
            ("GAME_TYPE".into(), game_type.into()),
        ]));
    }
    if reviewed(gid, "boost_flag") {
        command.insert("BET_BOOST".into(), "FALSE".into());
    }
    if gid == "945" {
        command.insert("EXTRA".into(), "FALSE".into());
    }
    match selected {
        Choice::Normal => {}
        Choice::Boost(index) => {
            ensure!(
                reviewed(gid, "boost_reviewed"),
                "Betsoft boost contract unavailable"
            );
            if gid == "1015" {
                positive_number(
                    field(cfg, "BOOST_BET")?
                        .split('|')
                        .nth(index)
                        .context("Betsoft boost option absent")?,
                )?;
            } else {
                ensure!(index == 0, "Betsoft boost option absent");
                let multiplier = if gid == "945" {
                    field(cfg, "EXTRA_MULT")?
                } else {
                    cfg.get("BET_BOOST_MULTIPLIER")
                        .or_else(|| cfg.get("BET_BOOST_MULT"))
                        .map(String::as_str)
                        .context("Betsoft boost multiplier absent")?
                };
                positive_number(multiplier)?;
            }
            if gid == "945" {
                command.insert("EXTRA".into(), "TRUE".into());
            } else {
                command.insert("BET_BOOST".into(), "TRUE".into());
                if matches!(gid, "1010" | "1019") {
                    command.remove("BUY");
                } else {
                    command.insert("BET_BOOST_TYPE".into(), index.to_string());
                }
            }
        }
        Choice::Buy(index) => {
            purchase_price(gid, cfg, index)?;
            if !matches!(gid, "794" | "796" | "798" | "801" | "800" | "945" | "1009") {
                command.insert("BUYFEATURETYPE".into(), index.to_string());
            }
            if gid == "934" {
                let counts: Vec<_> = field(cfg, "BUY_FS_COUNT")?.split('|').collect();
                if let Some(count) = counts.get(index) {
                    command.insert("BUYFEATURETYPE".into(), "0".into());
                    command.insert("BUYFEATUREFSCOUNT".into(), (*count).into());
                } else {
                    command.insert("BUYFEATURETYPE".into(), "1".into());
                }
            }
        }
    }
    Ok(command)
}

fn next_command(gid: &str, mode: &Mode, response: &Form) -> Result<Option<&'static str>> {
    let state = response.get("STATE").map(String::as_str);
    if mode.kind == ModeKind::Normal {
        let done = match gid {
            "2" => (1..=3).all(|i| response.contains_key(&format!("SLOT{i}"))),
            "426" | "478" | "500" => state == Some("MAIN_STATE"),
            "158" => {
                state.is_none()
                    && response.get("BONUS").map(String::as_str) == Some("0")
                    && response.get("SHIELD").map(String::as_str) == Some("0")
            }
            "548" => state.is_none() && response.get("PAYOUTS").map(String::as_str) == Some(""),
            "865" => {
                matches!(state, None | Some("MAIN"))
                    && response.get("STATE_NEXT").map(String::as_str) == Some("MAIN")
            }
            _ => matches!(state, Some("MAIN" | "0")),
        };
        if done {
            return Ok(None);
        }
        // Existing explicit Rooks Revenge continuation, absent from the general mock harvester.
        if gid == "277" && state == Some("1") {
            return Ok(Some("DOBONUS"));
        }
    } else if state == Some("MAIN") {
        return Ok(None);
    }
    if state.is_some_and(|s| {
        BINDINGS["named_continuations"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(s)))
    }) {
        Ok(Some(if gid == "402" { "COLLAPSE" } else { "DOBONUS" }))
    } else {
        Err(unknown_event(format!(
            "Betsoft {gid} unrecognized native state {state:?}"
        )))
    }
}

struct NativeResponse {
    fields: Form,
    raw: String,
}

fn next_response_command(
    gid: &str,
    mode: &Mode,
    response: &NativeResponse,
    previous_raw: Option<&str>,
) -> Result<Option<&'static str>> {
    let next = next_command(gid, mode, &response.fields)?;
    // The native harvester stops unchanged pending responses, ignoring only TIME.
    // Its explicit 402/COLLAPSE exception is preserved; this guard covers DOBONUS.
    // RunContext has already journaled both raw bodies before this decision.
    if next == Some("DOBONUS")
        && previous_raw.is_some_and(|previous| {
            NATIVE_TIME.replace_all(previous, "") == NATIVE_TIME.replace_all(&response.raw, "")
        })
    {
        return Err(unknown_event(format!(
            "Betsoft {gid} repeated pending response without native progress"
        )));
    }
    Ok(next)
}

#[cfg(test)]
fn settlement(gid: &str, response: &Form) -> Result<Option<()>> {
    let normal = Mode {
        key: "normal".into(),
        kind: ModeKind::Normal,
        label: String::new(),
        requires_prerequisites: Value::Null,
    };
    Ok(next_command(gid, &normal, response)?
        .is_none()
        .then_some(()))
}

async fn send(
    ctx: &RunContext,
    endpoint: &str,
    sid: &str,
    sequence: usize,
    mut fields: Form,
    label: &str,
    round_action: bool,
) -> Result<NativeResponse> {
    fields.extend([
        ("SID".into(), sid.into()),
        ("TIME".into(), now_ms().to_string()),
        ("CREQUESTID".into(), sequence.to_string()),
    ]);
    let body = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(&fields)
        .finish();
    let reply = ctx
        .http(
            label,
            "POST",
            endpoint,
            BTreeMap::from([(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            )]),
            body,
            round_action,
        )
        .await?;
    ensure!(
        reply.status == 200,
        "Betsoft HTTP {} at {label}",
        reply.status
    );
    let result = form(&reply.body)?;
    ensure!(
        field(&result, "RESULT")? == "OK",
        "Betsoft native command rejected at {label}"
    );
    Ok(NativeResponse {
        fields: result,
        raw: reply.body,
    })
}
fn check_game(gid: &str, response: &Form) -> Result<()> {
    if response.get("GID").is_some_and(|v| v != gid) {
        return Err(unknown_event("Betsoft response game changed"));
    }
    Ok(())
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    ensure!(
        supports(game, mode),
        "Betsoft slot/mode binding unavailable"
    );
    let gid = &game.id;
    let (host, bank) = if gid == "1009" {
        ("lobby-gp3demo", 14425)
    } else {
        ("democasino", 675)
    };
    let entry = Url::parse(&format!(
        "https://{host}.betsoftgaming.com/cwguestlogin.do?bankId={bank}&CDN=AUTO&gameId={gid}"
    ))?;
    let mut response = ctx.get("public_guest_launch", entry.as_str()).await?;
    if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
        let target = entry.join(
            response
                .headers
                .get("location")
                .context("Betsoft redirect missing location")?,
        )?;
        ensure!(
            target.origin() == entry.origin() && target.path() == "/free/base/template.jsp",
            "unexpected Betsoft demo redirect"
        );
        response = ctx.get("public_guest_template", target.as_str()).await?;
    }
    ensure!(
        response.status == 200,
        "unexpected Betsoft launcher response"
    );
    let pattern = Regex::new(r#"return "(SERVLET_URL=[^"]+)""#)?;
    let captures = pattern
        .captures(&response.body)
        .ok_or_else(|| unknown_event("Betsoft legacy launcher fields missing"))?;
    let launch = form(&captures[1])?;
    if field(&launch, "GAMEID")? != gid {
        return Err(unknown_event("Betsoft launcher game identity changed"));
    }
    let endpoint = Url::parse(field(&launch, "SERVLET_URL")?)?;
    let stem = endpoint
        .path()
        .strip_prefix('/')
        .and_then(|p| p.strip_suffix(".game"));
    ensure!(
        endpoint.origin().ascii_serialization() == "https://games-gp3demo.betsoftgaming.com"
            && endpoint.query().is_none()
            && endpoint.fragment().is_none()
            && stem.is_some_and(|p| !p.is_empty()
                && p.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))),
        "unexpected Betsoft native endpoint"
    );
    let sid = field(&launch, "SID")?;
    ensure!(!sid.is_empty(), "Betsoft public guest SID missing");
    let init = send(
        ctx,
        endpoint.as_str(),
        sid,
        1,
        Form::from([("CMD".into(), "ENTER".into())]),
        "enter",
        false,
    )
    .await?;
    let init = init.fields;
    ensure!(
        field(&init, "game_mode")? == "free",
        "Betsoft public FREE session not confirmed"
    );
    check_game(gid, &init)?;
    if init
        .get("STATE")
        .is_some_and(|s| !matches!(s.as_str(), "0" | "MAIN" | "MAIN_STATE"))
    {
        return Err(unknown_event(
            "Betsoft fresh init is not a known idle state",
        ));
    }
    let coin = coin_index(&init, mode)?;
    let mut command = build_command(gid, &init, mode, coin)?;
    ctx.save_session(&json!({"provider":"betsoft","game":gid,"sid":sid,"endpoint":endpoint.as_str(),"playMode":"free"}))?;
    let mut previous_raw = None;
    for step in 0..ctx.max_steps() {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let frame = send(
            ctx,
            endpoint.as_str(),
            sid,
            step + 2,
            command,
            if step == 0 {
                "place_bet"
            } else {
                "continuation"
            },
            true,
        )
        .await?;
        check_game(gid, &frame.fields)?;
        if let Some(next) = next_response_command(gid, mode, &frame, previous_raw.as_deref())? {
            command = Form::from([("CMD".into(), next.into())]);
            previous_raw = Some(frame.raw);
        } else {
            return Ok(Completion {
                native_round_id: None,
                details: json!({"game":gid,"mode":mode.key,"steps":step+1}),
            });
        }
    }
    bail!("Betsoft continuation limit reached")
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn terminal_native_reels_and_states_ignore_money() {
        assert!(
            settlement("2", &form("SLOT1=1&SLOT2=2&SLOT3=3").unwrap())
                .unwrap()
                .is_some()
        );
        assert!(
            settlement("266", &form("STATE=0").unwrap())
                .unwrap()
                .is_some()
        );
        assert!(
            settlement("277", &form("STATE=1").unwrap())
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn unrecognized_state_is_typed() {
        assert!(
            settlement("277", &form("STATE=99").unwrap())
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}

#[cfg(test)]
mod recovered_tests {
    use super::*;
    use serde_json::Value;

    fn mode(key: &str, kind: ModeKind) -> Mode {
        Mode {
            key: key.into(),
            kind,
            label: String::new(),
            requires_prerequisites: Value::Null,
        }
    }
    #[test]
    fn buy_fs_count_and_coin_restriction_follow_fresh_native_offer() {
        let init = form("DEFAULTNUMLINES=20&IBETVALUES=0.01 0.02 0.05&BUY_FS_COUNT=5|10&BUYPRICE=100|200&BUY_RESPIN_PRICE=300&BUY_FEATURE_ALLOWED_COINS=0.02 0.05|0.05|0.01").unwrap();
        let m = mode("1", ModeKind::Purchase);
        assert_eq!(coin_index(&init, &m).unwrap(), 2);
        let cmd = build_command("934", &init, &m, 2).unwrap();
        assert_eq!(cmd["BUYFEATURETYPE"], "0");
        assert_eq!(cmd["BUYFEATUREFSCOUNT"], "10");
        assert!(build_command("934", &init, &mode("3", ModeKind::Purchase), 0).is_err());
    }
    #[test]
    fn legacy_variants_and_boosts_use_their_exact_commands() {
        let cfg = form("DEFAULTNUMLINES=25&DEF_GAME_TYPE=both&BUYPRICE=50&EXTRA_MULT=2&BET_BOOST_MULT=2&BOOST_BET=40|70").unwrap();
        assert_eq!(
            build_command("426", &cfg, &mode("normal", ModeKind::Normal), 0).unwrap()["GAME_TYPE"],
            "both"
        );
        assert_eq!(
            build_command("1015", &cfg, &mode("boost-1", ModeKind::Booster), 0).unwrap()["BET_BOOST_TYPE"],
            "1"
        );
        assert_eq!(
            build_command("945", &cfg, &mode("boost", ModeKind::Booster), 0).unwrap()["EXTRA"],
            "TRUE"
        );
        assert!(
            !build_command("945", &cfg, &mode("0", ModeKind::Purchase), 0)
                .unwrap()
                .contains_key("BUYFEATURETYPE")
        );
    }
    #[test]
    fn only_known_states_produce_continuations_without_wallet_fields() {
        let m = mode("0", ModeKind::Purchase);
        assert_eq!(
            next_command("1000", &m, &form("STATE=BUY_HOLDWIN").unwrap()).unwrap(),
            Some("DOBONUS")
        );
        assert_eq!(
            next_command("1000", &m, &form("STATE=MAIN").unwrap()).unwrap(),
            None
        );
        assert!(
            next_command("1000", &m, &form("STATE=NEW_PICK_FEATURE").unwrap())
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
        assert!(next_command("1000", &m, &form("STATE=1").unwrap()).is_err());
    }

    #[test]
    fn bare_acknowledgement_does_not_complete_unproven_stateless_games() {
        let normal = mode("normal", ModeKind::Normal);
        for gid in ["692", "818"] {
            assert!(
                next_command(gid, &normal, &form("RESULT=OK").unwrap())
                    .unwrap_err()
                    .downcast_ref::<crate::model::UnknownEvent>()
                    .is_some()
            );
            let game = Game {
                id: gid.into(),
                name: String::new(),
                modes: vec![normal.clone()],
            };
            assert!(!supports(&game, &normal));
        }
    }

    #[test]
    fn repeated_pending_response_ignores_time_and_stops_before_another_command() {
        let purchase = mode("0", ModeKind::Purchase);
        let previous = "RESULT=OK&STATE=BUY_HOLDWIN&RESPINS=3&TIME=1700000000000\r\n";
        let raw = "RESULT=OK&STATE=BUY_HOLDWIN&RESPINS=3&TIME=1700000001000\r\n";
        let frame = NativeResponse {
            fields: form(raw).unwrap(),
            raw: raw.into(),
        };
        assert_eq!(
            next_response_command("1000", &purchase, &frame, None).unwrap(),
            Some("DOBONUS")
        );
        assert!(
            next_response_command("1000", &purchase, &frame, Some(previous))
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
        // Progress detection must leave the exact received bytes available for the journal.
        assert_eq!(frame.raw, raw);

        let advanced = "RESULT=OK&STATE=BUY_HOLDWIN&RESPINS=2&TIME=1700000001000\r\n";
        let frame = NativeResponse {
            fields: form(advanced).unwrap(),
            raw: advanced.into(),
        };
        assert_eq!(
            next_response_command("1000", &purchase, &frame, Some(previous)).unwrap(),
            Some("DOBONUS")
        );
        let terminal = "RESULT=OK&STATE=MAIN&TIME=1700000001000\r\n";
        let frame = NativeResponse {
            fields: form(terminal).unwrap(),
            raw: terminal.into(),
        };
        assert_eq!(
            next_response_command("1000", &purchase, &frame, Some(terminal)).unwrap(),
            None
        );
    }
}
