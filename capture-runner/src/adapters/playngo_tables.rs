//! Eighteen source-pinned native table/draw/classic contracts, ported from
//! playngo_{craps,holdem,roulette,draw_games,legacy_tables}.py. Public provenance
//! is embedded in playngo-tables.json; no evidence or old runner is used at runtime.
use super::{Command, NativeSocket, RunContext, latest, native, send};
use crate::model::{Completion, Game, Mode, ModeKind, unknown_event};
use anyhow::{Result, ensure};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::LazyLock};

static BINDINGS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../bindings/playngo-tables.json"))
        .expect("compiled Play'n GO table bindings")
});

pub(super) fn embedded_settings(game: &str) -> Option<Value> {
    BINDINGS["games"].get(game).cloned()
}
pub(super) fn supports(game: &Game, mode: &Mode) -> bool {
    mode.kind == ModeKind::Normal
        && mode.key == "normal"
        && BINDINGS["games"].get(&game.id).is_some()
}

pub(super) fn validate_binding(game: &Game, settings: &Value) -> Result<()> {
    let binding = &BINDINGS["games"][&game.id];
    require(
        settings["gid"] == binding["gid"] && settings["source_sha256"] == binding["source_sha256"],
        "configured GID/client hash differs from reviewed table binding",
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum Family {
    Craps,
    ThreeHandHoldem,
    Roulette,
    Poker,
    Wheel,
    Keno,
    SimpleSlot,
    Blackjack,
    CasinoHoldem,
    Baccarat,
}
#[derive(Debug)]
struct Profile {
    family: Family,
    denomination: u64,
    entry: String,
}
fn require(condition: bool, message: &str) -> Result<()> {
    if !condition {
        return Err(unknown_event(format!("Play'n GO table: {message}")));
    }
    Ok(())
}
fn integer(raw: &str) -> Result<u64> {
    native(raw).map_err(|e| unknown_event(format!("Play'n GO table integer: {e}")))
}
fn args(items: &[Command], code: u64) -> Result<&[String]> {
    latest(items, code)
        .ok_or_else(|| unknown_event(format!("Play'n GO table native command {code} missing")))
}
fn idle(items: &[Command]) -> bool {
    latest(items, 52)
        .and_then(|v| v.get(1))
        .is_some_and(|state| state == "0")
}
fn reject(items: &[Command]) -> Result<()> {
    ensure!(
        !items.iter().any(|x| matches!(x.code, 110 | 111)),
        "Play'n GO table provider rejection"
    );
    Ok(())
}
fn ordinary_denominations(items: &[Command]) -> Result<Vec<u64>> {
    let chips = args(items, 54)?;
    let count = integer(chips.first().map(String::as_str).unwrap_or(""))? as usize;
    require(
        count > 0 && count <= 4096 && chips.len() > count,
        "unrecognized denomination menu",
    )?;
    let values = chips[1..=count]
        .iter()
        .map(|v| integer(v))
        .collect::<Result<Vec<_>>>()?;
    require(
        values.iter().all(|v| *v > 0),
        "nonpositive request denomination",
    )?;
    Ok(values)
}
fn poker_denominations(items: &[Command]) -> Result<Vec<u64>> {
    let chips = args(items, 54)?;
    let groups = integer(chips.first().map(String::as_str).unwrap_or(""))? as usize;
    require(groups > 0 && groups <= 128, "unrecognized poker hand menu")?;
    let mut at = 1;
    let mut menus = BTreeMap::new();
    for _ in 0..groups {
        let header = chips
            .get(at..at + 2)
            .ok_or_else(|| unknown_event("Play'n GO poker denomination group truncated"))?;
        let hands = integer(&header[0])?;
        let count = integer(&header[1])? as usize;
        at += 2;
        require(
            hands > 0 && count > 0 && count <= 4096,
            "unrecognized poker denomination count",
        )?;
        let values = chips
            .get(at..at + count)
            .ok_or_else(|| unknown_event("Play'n GO poker denomination group truncated"))?
            .iter()
            .map(|v| integer(v))
            .collect::<Result<Vec<_>>>()?;
        require(
            values.iter().all(|v| *v > 0) && menus.insert(hands, values).is_none(),
            "invalid or duplicate poker hand group",
        )?;
        at += count;
    }
    require(at == chips.len(), "poker denomination trailing fields")?;
    menus
        .remove(&1)
        .ok_or_else(|| unknown_event("Play'n GO one-hand poker not published"))
}
fn tokens(family: Family) -> &'static [&'static str] {
    match family {
        Family::Craps => &["onClientRoll", "PASS"],
        Family::ThreeHandHoldem => &[".SEAT_CNT=3", ".FOLD=0", ".DEAL=1", ".PLAY=2", ".TURN=5"],
        Family::Roulette => &[".CLIENT_SPIN=1"],
        Family::Poker => &[
            ".DEAL=1",
            ".DRAW=2",
            ".COLLECT=3",
            "String(t.coins),String(t.denom),String(t.hands)",
            "t.holdMask&1<<",
        ],
        Family::Wheel => &[
            ".CSPIN=1",
            ".SSPIN=1",
            "r=new Array(7)",
            "e.getInt(10)",
            "t.bets[i].toString()",
        ],
        Family::Keno => &[
            ".SPINEND,[]",
            "String(this._model.roundHandler.bet),String(1),String(e.length)",
            "this._model.pickedNumbers",
        ],
        Family::SimpleSlot => &[
            "createLogicCommandConverter(){var t=this._config;new JA(",
            "class JA extends Qy",
            "[String(t.coins),String(t.denom)]",
            "e.getInt(6)/e.getInt(1),s=e.getInt(6)",
        ],
        Family::Blackjack => &[
            ".DEAL=1",
            ".ACTION=2",
            ".STAND=2",
            ".INSURANCENO=128",
            ".TURN=5",
            ".OUTCOME=6",
            ".ROUNDEND=7",
            "e[0]=t.currentHand.toString(),e[1]=t.action.toString(),e[2]=t.seat.toString()",
        ],
        Family::CasinoHoldem => &[
            ".DEAL=1",
            ".PLAY=2",
            ".OUTCOME=3",
            "t.anteBet.toString()",
            "t.aaBet.toString()",
            "t.play?\"1\":\"0\"",
        ],
        Family::Baccarat => &[
            ".DEAL=1",
            ".OUTCOME=3",
            "t.playerBet.toString()",
            "t.bankerBet.toString()",
            "t.tieBet.toString()",
        ],
    }
}
fn profile(family: Family, source: &str, initial: &[Command]) -> Result<Profile> {
    reject(initial)?;
    require(idle(initial), "initial native state is not idle")?;
    require(
        tokens(family).iter().all(|token| source.contains(token)),
        "reviewed public converter changed",
    )?;
    if family == Family::Craps {
        let state = args(initial, 56)?;
        require(
            state.get(..3) == Some(&["0".into(), "0".into(), "0".into()][..]),
            "existing Craps bet cannot be reused",
        )?;
    }
    if family == Family::Roulette {
        // The reviewed native position ID, not a UI coordinate or a guessed index.
        require(Regex::new(r"case 150:return\{position:\w+,type:0,index:0,seatIdx:0,betIdx:0,contains:\[0\],payout:([\w$]+)\.TYPE_PAYOUTS\[0\]\}")?.is_match(source), "straight-zero position converter missing")?;
    }
    if family == Family::Keno {
        require(
            Regex::new(r"onSpin\(t\)\{var e=this\._model\.pickedNumbers")?.is_match(source),
            "Keno converter binding missing",
        )?;
    }
    let values = if family == Family::Poker {
        poker_denominations(initial)?
    } else {
        ordinary_denominations(initial)?
    };
    let denomination = *values.iter().min().unwrap();
    let limit_type = match family {
        Family::ThreeHandHoldem | Family::Wheel | Family::CasinoHoldem => Some("4"),
        Family::Roulette => Some("6"),
        Family::Blackjack => Some("2"),
        Family::Baccarat => Some("8"),
        _ => None,
    };
    if family == Family::Craps {
        let limits = args(initial, 53)?;
        require(
            limits
                .get(1)
                .is_some_and(|v| v == &denomination.to_string()),
            "Craps native minimum selector changed",
        )?;
    }
    if let Some(expected) = limit_type {
        let limits = args(initial, 53)?;
        require(
            limits.first().map(String::as_str) == Some(expected),
            "unknown table limits family",
        )?;
        require(
            limits
                .get(1)
                .is_some_and(|v| v == &denomination.to_string()),
            "native table minimum differs from chip selector",
        )?;
        let slots: &[usize] = if family == Family::Roulette {
            &[3, 5]
        } else if family == Family::Wheel {
            &[3]
        } else {
            &[]
        };
        require(
            slots.iter().all(|i| {
                limits
                    .get(*i)
                    .is_some_and(|v| v == &denomination.to_string())
            }),
            "native selected position minimum missing",
        )?;
    }
    let entry = match family {
        Family::Craps => format!("1 1 0 {denomination} 0"),
        Family::ThreeHandHoldem => format!("1 3 {denomination} 0 0 0 0 0"),
        Family::Roulette => format!("1 1 150 {denomination}"),
        Family::Poker => format!("1 1 {denomination} 1"),
        Family::Wheel => format!("1 {denomination} 0 0 0 0 0 0"),
        Family::Keno => format!("1 {denomination} 1 2 1 2"),
        // The exact source hash binds these three clients to the reference's
        // verified one coin / one line profile and old coins/denomination wire.
        Family::SimpleSlot => format!(
            "1 1 {denomination}{}",
            if latest(initial, 83).is_some() {
                " 1"
            } else {
                ""
            }
        ),
        Family::Blackjack | Family::Baccarat => format!("1 {denomination} 0 0"),
        Family::CasinoHoldem => format!("1 {denomination} 0"),
    };
    Ok(Profile {
        family,
        denomination,
        entry,
    })
}

fn poker_draw(items: &[Command]) -> Result<Option<(u64, bool)>> {
    if let Some(draw) = latest(items, 2) {
        require(draw.len() >= 8, "poker draw shape changed")?;
        require(
            matches!(draw[7].as_str(), "0" | "1"),
            "poker collection flag changed",
        )?;
        return Ok(Some((integer(&draw[6])?, draw[7] == "1")));
    }
    if let Some(draw) = latest(items, 6) {
        require(
            draw.len() == 11 && draw.get(..3) == Some(&["0".into(), "5".into(), "1".into()][..]),
            "poker hand is not one unheld hand",
        )?;
        require(
            matches!(draw[10].as_str(), "0" | "1"),
            "poker collection flag changed",
        )?;
        return Ok(Some((integer(&draw[9])?, draw[10] == "1")));
    }
    Ok(None)
}
fn craps_next(items: &[Command]) -> Result<Option<String>> {
    let answer = args(items, 1)?;
    args(items, 3)?; // Native roll-end opcode; its monetary fields are not audited.
    let dice = integer(answer.first().map(String::as_str).unwrap_or(""))? as usize;
    require(dice == 2, "unrecognized Craps dice count")?;
    let values = answer
        .get(1..1 + dice)
        .ok_or_else(|| unknown_event("Play'n GO Craps dice missing"))?;
    require(
        values
            .iter()
            .all(|v| integer(v).is_ok_and(|n| (1..=6).contains(&n))),
        "unrecognized Craps dice",
    )?;
    let count = answer
        .get(1 + dice)
        .ok_or_else(|| unknown_event("Play'n GO Craps active count missing"))?;
    let count = integer(count)? as usize;
    require(count <= 1, "unexpected active Craps bet count")?;
    let pairs = answer
        .get(2 + dice..2 + dice + count * 2)
        .ok_or_else(|| unknown_event("Play'n GO Craps active bet truncated"))?;
    let mut active = false;
    for pair in pairs.as_chunks::<2>().0 {
        require(pair[0] == "0", "unrecognized active Craps position")?;
        active = true;
    }
    for feature in items.iter().filter(|x| x.code == 2) {
        require(feature.args.len() >= 2, "Craps feature truncated")?;
        let kind = integer(&feature.args[0])?;
        let n = integer(&feature.args[1])? as usize;
        require(
            matches!(kind, 0..=2) && n <= 1,
            "unknown Craps feature or transfer",
        )?;
        if kind == 2 {
            require(n == 0, "unrecognized Craps transfer")?;
            continue;
        }
        require(
            feature.args.len() == 2 + n * 2,
            "Craps feature count changed",
        )?;
        for pair in feature.args[2..].as_chunks::<2>().0 {
            require(pair[0] == "0", "unexpected Craps feature position")?;
            if kind == 1 {
                active = false;
            }
        }
    }
    if !active && idle(items) {
        return Ok(None);
    }
    require(
        active && !idle(items),
        "Craps active state conflicts with idle marker",
    )?;
    Ok(Some("1 0 0".into()))
}
fn next_command(p: &Profile, items: &[Command]) -> Result<Option<String>> {
    reject(items)?;
    if p.family == Family::Craps {
        return craps_next(items);
    }
    if idle(items) {
        return Ok(None);
    }
    let next = match p.family {
        Family::ThreeHandHoldem => {
            let deal = args(items, 1)?;
            require(
                deal.len() >= 22 && deal[0] == "3",
                "three-hand Hold'em deal shape changed",
            )?;
            for seat in 0..3 {
                let at = 1 + seat * 6;
                let expected = if seat == 0 {
                    p.denomination.to_string()
                } else {
                    "0".into()
                };
                require(
                    deal[at] == expected && deal[at + 1] == "0",
                    "unexpected occupied seat or AA selection",
                )?;
            }
            require(
                args(items, 5)?.first().is_some_and(|s| s == "0"),
                "three-hand Hold'em turn changed",
            )?;
            "2 0 0".into()
        }
        Family::Blackjack => {
            if latest(items, 8).is_some() {
                "3 0 0 0".into()
            } else {
                let turn = args(items, 5)?;
                require(turn.len() == 3, "blackjack turn shape changed")?;
                let (hand, mask, seat) =
                    (integer(&turn[0])?, integer(&turn[1])?, integer(&turn[2])?);
                require(
                    hand == 0 && seat == 0,
                    "blackjack selected seat/hand changed",
                )?;
                if mask & 128 != 0 {
                    "2 0 128 0".into()
                } else if mask & 2 != 0 {
                    "2 0 2 0".into()
                } else {
                    return Err(unknown_event(
                        "Play'n GO blackjack Stand/decline insurance not offered",
                    ));
                }
            }
        }
        Family::CasinoHoldem => {
            let deal = args(items, 1)?;
            require(
                deal.len() >= 9 && deal[0] == p.denomination.to_string() && deal[1] == "0",
                "casino Hold'em deal selector changed",
            )?;
            "2 0".into()
        }
        Family::Poker => {
            if let Some(deal) = latest(items, 1) {
                require(
                    deal.len() >= 9
                        && deal[0] == "1"
                        && deal[1] == p.denomination.to_string()
                        && deal[8] == "1",
                    "poker one-hand deal changed",
                )?;
                "2 0 0 0 0 0".into()
            } else if let Some((pending, collect)) = poker_draw(items)? {
                require(pending > 0, "poker draw has no known pending collection")?;
                if collect { "3".into() } else { "52".into() }
            } else {
                return Err(unknown_event("Play'n GO unknown poker continuation"));
            }
        }
        Family::Keno => {
            if let Some(feature) = latest(items, 2) {
                require(
                    feature.len() == 24 && feature[0] == "0" && feature[3] == "20",
                    "Keno ball count changed",
                )?;
                let balls = feature[4..]
                    .iter()
                    .map(|v| integer(v))
                    .collect::<Result<std::collections::BTreeSet<_>>>()?;
                require(
                    balls.len() == 20 && balls.iter().all(|n| (1..=80).contains(n)),
                    "Keno ball range or duplicates changed",
                )?;
                if let Some(echo) = latest(items, 1) {
                    require(
                        echo.len() >= 2
                            && echo[0] == p.denomination.to_string()
                            && echo.get(2..) == Some(&["2".into(), "1".into(), "2".into()][..]),
                        "Keno selected numbers changed",
                    )?;
                }
                "3".into()
            } else if latest(items, 3)
                .is_some_and(|a| a.len() >= 5 && integer(&a[2]).is_ok_and(|n| n > 0))
            {
                "4 0".into()
            } else {
                return Err(unknown_event("Play'n GO unknown Keno continuation"));
            }
        }
        Family::SimpleSlot => {
            let result = args(items, 2)?;
            require(
                result.len() == 7 && integer(&result[6])? > 0,
                "classic slot collection not offered",
            )?;
            "4 0".into()
        }
        _ => {
            return Err(unknown_event(
                "Play'n GO table continuation not documented for this family",
            ));
        }
    };
    Ok(Some(next))
}

fn closed(p: &Profile, all: &[Command]) -> Result<()> {
    reject(all)?;
    require(idle(all), "native idle end state missing")?;
    // Check native result/selector identities, without parsing awards or wallet fields.
    match p.family {
        Family::Craps => {
            require(craps_next(all)?.is_none(), "Craps bet remains active")?;
        }
        Family::ThreeHandHoldem => {
            let answer = args(all, 2)?;
            require(
                answer.get(..3) == Some(&["0".into(), "0".into(), "0".into()][..]),
                "Hold'em selected action is not seat-zero Fold",
            )?;
            require(args(all, 3)?.len() >= 25, "Hold'em outcome shape changed")?;
        }
        Family::Roulette => {
            let spin = args(all, 1)?;
            require(
                spin.len() >= 4
                    && integer(&spin[0])? <= 36
                    && spin[1] == p.denomination.to_string(),
                "roulette accepted selector changed",
            )?;
            let count = integer(&spin[3])? as usize;
            require(
                count <= 1 && spin.len() >= 4 + count * 2,
                "roulette result positions changed",
            )?;
            if count == 1 {
                require(spin[4] == "150", "roulette result position changed")?;
            }
        }
        Family::Wheel => {
            let result = args(all, 1)?;
            require(
                result.len() >= 10
                    && result[3] == p.denomination.to_string()
                    && result[4..10].iter().all(|s| s == "0"),
                "wheel accepted position changed",
            )?;
        }
        Family::SimpleSlot => {
            let result = args(all, 2)?;
            require(
                result.len() >= 6 && result[0] == "1" && result[1] == p.denomination.to_string(),
                "classic slot coin/denomination echo changed",
            )?;
        }
        Family::Poker => {
            if let Some(draw) = latest(all, 2) {
                require(draw.len() >= 8, "poker draw result missing")?;
            } else {
                let draw = args(all, 6)?;
                require(
                    draw.len() == 11
                        && draw.get(..3) == Some(&["0".into(), "5".into(), "1".into()][..]),
                    "poker one-hand result changed",
                )?;
            }
        }
        Family::Keno => {
            let feature = args(all, 2)?;
            let end = args(all, 3)?;
            require(
                !feature.is_empty()
                    && feature[0] == "0"
                    && !end.is_empty()
                    && end.len() <= 5
                    && end.get(3).is_none_or(|s| s == "0"),
                "Keno final state changed",
            )?;
        }
        Family::Blackjack => {
            let deal = args(all, 1)?;
            require(
                deal.get(..3) == Some(&[p.denomination.to_string(), "0".into(), "0".into()][..]),
                "blackjack accepted seat selection changed",
            )?;
            let outcomes: Vec<_> = all.iter().filter(|x| x.code == 6).collect();
            require(
                outcomes.len() == 1
                    && outcomes[0].args.len() == 4
                    && outcomes[0].args[0] == "0"
                    && outcomes[0].args[3] == "0"
                    && latest(all, 7).is_some(),
                "blackjack round-end identity changed",
            )?;
        }
        Family::CasinoHoldem => {
            require(
                args(all, 2)?.get(..2) == Some(&["0".into(), "0".into()][..])
                    && args(all, 3)?.len() >= 12,
                "casino Hold'em Fold outcome changed",
            )?;
        }
        Family::Baccarat => {
            require(
                args(all, 1)?.get(..3)
                    == Some(&[p.denomination.to_string(), "0".into(), "0".into()][..])
                    && args(all, 3)?.len() == 4,
                "baccarat player-only result changed",
            )?;
        }
    }
    Ok(())
}

pub(super) async fn execute(
    ctx: &RunContext,
    socket: &mut NativeSocket,
    number: &mut u64,
    session: &str,
    game: &Game,
    source: &str,
    initial: &[Command],
) -> Result<Completion> {
    let family: Family = serde_json::from_value(BINDINGS["games"][&game.id]["family"].clone())?;
    let p = profile(family, source, initial)?;
    ctx.note(
        "playngo_table_selector",
        &json!({"game":game.id,"family":family,"entry":p.entry,"mode":"normal"}),
    )?;
    let mut command = p.entry.clone();
    let mut all = Vec::new();
    let limit = ctx
        .max_steps()
        .min(if family == Family::Craps { 100 } else { 10 });
    for _ in 0..limit {
        let response = send(
            ctx,
            socket,
            number,
            session,
            &command,
            "native_table_action",
            true,
        )
        .await?;
        let next = next_command(&p, &response)?;
        all.extend(response);
        if let Some(next) = next {
            command = next;
        } else {
            closed(&p, &all)?;
            return Ok(Completion {
                native_round_id: None,
                details: json!({"game":game.id,"mode":"normal","family":family,"entry":p.entry,"native_responses":all}),
            });
        }
    }
    Err(unknown_event(
        "Play'n GO native table continuation budget exhausted",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(code: u64, values: &[&str]) -> Command {
        Command {
            code,
            args: values.iter().map(|v| (*v).into()).collect(),
        }
    }
    fn state() -> Command {
        c(52, &["omitted-wallet", "0", "omitted-bonus-wallet"])
    }
    fn initial(family: Family) -> Vec<Command> {
        let kind = match family {
            Family::Roulette => "6",
            Family::Blackjack => "2",
            Family::Baccarat => "8",
            _ => "4",
        };
        vec![
            if family == Family::Poker {
                c(54, &["2", "1", "2", "50", "25", "3", "1", "100"])
            } else {
                c(54, &["2", "50", "25"])
            },
            c(53, &[kind, "25", "1000", "25", "1000", "25"]),
            c(56, &["0", "0", "0"]),
            state(),
        ]
    }
    fn source(family: Family) -> String {
        let mut source = tokens(family).join(";");
        if family == Family::Roulette {
            source.push_str(";case 150:return{position:p,type:0,index:0,seatIdx:0,betIdx:0,contains:[0],payout:q.TYPE_PAYOUTS[0]}");
        }
        if family == Family::Keno {
            source.push_str(";onSpin(t){var e=this._model.pickedNumbers");
        }
        source
    }
    fn p(family: Family) -> Profile {
        profile(family, &source(family), &initial(family)).unwrap()
    }
    fn unknown(error: anyhow::Error) {
        assert!(
            error.downcast_ref::<crate::model::UnknownEvent>().is_some(),
            "{error:#}"
        );
    }

    #[test]
    fn all_eighteen_advertised_bindings_have_exact_reference_entry_selectors() {
        let expected = [
            ("gocraps", "1 1 0 25 0"),
            ("threehandholdem", "1 3 25 0 0 0 0 0"),
            ("europeanroulette", "1 1 150 25"),
            ("jokerpoker", "1 1 25 1"),
            ("deuceswild", "1 1 25 1"),
            ("jacksorbetter", "1 1 25 1"),
            ("superwheel", "1 25 0 0 0 0 0 0"),
            ("moneywheel", "1 25 0 0 0 0 0 0"),
            ("aceofspades", "1 1 25"),
            ("luckydiamonds", "1 1 25"),
            ("wildmelon", "1 1 25"),
            ("keno", "1 25 1 2 1 2"),
            ("doubleexposureblackjackmh", "1 25 0 0"),
            ("singledeckblackjackmh", "1 25 0 0"),
            ("blackjackmh", "1 25 0 0"),
            ("europeanblackjackmh", "1 25 0 0"),
            ("casinoholdem", "1 25 0"),
            ("minibaccarat", "1 25 0 0"),
        ];
        assert_eq!(BINDINGS["games"].as_object().unwrap().len(), 18);
        for (gid, command) in expected {
            let (id, binding) = BINDINGS["games"]
                .as_object()
                .unwrap()
                .iter()
                .find(|(_, b)| b["gid"] == gid)
                .unwrap();
            let family: Family = serde_json::from_value(binding["family"].clone()).unwrap();
            let mode = Mode {
                key: "normal".into(),
                kind: ModeKind::Normal,
                label: String::new(),
                requires_prerequisites: Value::Null,
            };
            let game = Game {
                id: id.clone(),
                name: binding["name"].as_str().unwrap().into(),
                modes: vec![mode.clone()],
            };
            assert!(super::super::supports(&game, &mode), "{gid}");
            let settings = super::super::embedded_settings(&game).unwrap();
            super::super::validate(&game, &mode, &settings).unwrap();
            let mut changed = settings.clone();
            changed["source_sha256"] = json!("0".repeat(64));
            unknown(super::super::validate(&game, &mode, &changed).unwrap_err());
            assert_eq!(settings["source_sha256"], binding["source_sha256"]);
            assert_eq!(p(family).entry, command, "{gid}");
        }
    }
    #[test]
    fn ragged_poker_denominations_and_classic_optional_flag_preserve_wire_types() {
        assert_eq!(p(Family::Poker).denomination, 25);
        let mut initial = initial(Family::SimpleSlot);
        initial.push(c(83, &[]));
        assert_eq!(
            profile(Family::SimpleSlot, &source(Family::SimpleSlot), &initial)
                .unwrap()
                .entry,
            "1 1 25 1"
        );
        let bad = vec![c(54, &["2", "1", "1", "25", "1", "1", "50"])];
        unknown(poker_denominations(&bad).unwrap_err());
    }
    #[test]
    fn craps_never_repeats_the_initial_placement_and_closes_without_wallet_values() {
        let p = p(Family::Craps);
        let active = vec![
            c(1, &["2", "3", "4", "1", "0", "25", "unread-tail"]),
            c(3, &["end", "omitted-payout"]),
        ];
        assert_eq!(next_command(&p, &active).unwrap().as_deref(), Some("1 0 0"));
        let done = vec![
            c(1, &["2", "3", "4", "1", "0", "25"]),
            c(2, &["1", "1", "0", "omitted-payout"]),
            c(3, &["end", "omitted-payout"]),
            state(),
        ];
        assert!(next_command(&p, &done).unwrap().is_none());
        closed(&p, &done).unwrap();
        unknown(
            next_command(
                &p,
                &[c(1, &["2", "3", "4", "1", "7", "25"]), c(3, &["end"])],
            )
            .unwrap_err(),
        );
    }
    #[test]
    fn blackjack_declines_insurance_and_stands_only_at_the_native_seat() {
        let p = p(Family::Blackjack);
        assert_eq!(
            next_command(&p, &[c(5, &["0", "130", "0"])])
                .unwrap()
                .as_deref(),
            Some("2 0 128 0")
        );
        assert_eq!(
            next_command(&p, &[c(5, &["0", "2", "0"])])
                .unwrap()
                .as_deref(),
            Some("2 0 2 0")
        );
        assert_eq!(
            next_command(&p, &[c(8, &["offer"])]).unwrap().as_deref(),
            Some("3 0 0 0")
        );
        let done = vec![
            c(1, &["25", "0", "0"]),
            c(6, &["0", "outcome", "omitted-payout", "0"]),
            c(7, &[]),
            state(),
        ];
        closed(&p, &done).unwrap();
        unknown(next_command(&p, &[c(5, &["1", "2", "0"])]).unwrap_err());
        unknown(next_command(&p, &[c(5, &["0", "1", "0"])]).unwrap_err());
    }
    #[test]
    fn holdem_folds_are_exact_and_no_additional_call_is_encoded() {
        let p = p(Family::ThreeHandHoldem);
        let mut deal = vec!["0".to_string(); 22];
        deal[0] = "3".into();
        deal[1] = "25".into();
        let items = vec![
            Command {
                code: 1,
                args: deal,
            },
            c(5, &["0"]),
        ];
        assert_eq!(next_command(&p, &items).unwrap().as_deref(), Some("2 0 0"));
        let done = vec![
            c(2, &["0", "0", "0"]),
            Command {
                code: 3,
                args: vec!["omitted".into(); 25],
            },
            state(),
        ];
        closed(&p, &done).unwrap();
        let p = profile(
            Family::CasinoHoldem,
            &source(Family::CasinoHoldem),
            &initial(Family::CasinoHoldem),
        )
        .unwrap();
        assert_eq!(
            next_command(&p, &[c(1, &["25", "0", "c", "c", "c", "c", "c", "c", "c"])])
                .unwrap()
                .as_deref(),
            Some("2 0")
        );
        closed(
            &p,
            &[
                c(2, &["0", "0"]),
                Command {
                    code: 3,
                    args: vec!["omitted".into(); 12],
                },
                state(),
            ],
        )
        .unwrap();
    }
    #[test]
    fn poker_draw_and_collect_variants_close_without_numeric_awards_or_balances() {
        let p = p(Family::Poker);
        assert_eq!(
            next_command(&p, &[c(1, &["1", "25", "c", "c", "c", "c", "c", "0", "1"])])
                .unwrap()
                .as_deref(),
            Some("2 0 0 0 0 0")
        );
        assert_eq!(
            next_command(&p, &[c(2, &["c", "c", "c", "c", "c", "flag", "100", "1"])])
                .unwrap()
                .as_deref(),
            Some("3")
        );
        assert_eq!(
            next_command(
                &p,
                &[c(
                    6,
                    &["0", "5", "1", "c", "c", "c", "c", "c", "flag", "100", "0"]
                )]
            )
            .unwrap()
            .as_deref(),
            Some("52")
        );
        closed(
            &p,
            &[
                c(2, &["c", "c", "c", "c", "c", "flag", "omitted-payout", "0"]),
                state(),
            ],
        )
        .unwrap();
        closed(
            &p,
            &[
                c(
                    6,
                    &[
                        "0",
                        "5",
                        "1",
                        "c",
                        "c",
                        "c",
                        "c",
                        "c",
                        "flag",
                        "omitted-payout",
                        "0",
                    ],
                ),
                state(),
            ],
        )
        .unwrap();
    }
    #[test]
    fn roulette_wheel_baccarat_and_classic_native_close_ignore_monetary_metadata() {
        for (family, items) in [
            (
                Family::Roulette,
                vec![c(1, &["7", "25", "omitted-payout", "0"]), state()],
            ),
            (
                Family::Wheel,
                vec![
                    c(
                        1,
                        &[
                            "result",
                            "result",
                            "result",
                            "25",
                            "0",
                            "0",
                            "0",
                            "0",
                            "0",
                            "0",
                            "omitted-payout",
                        ],
                    ),
                    state(),
                ],
            ),
            (
                Family::Baccarat,
                vec![
                    c(1, &["25", "0", "0"]),
                    c(3, &["outcome", "omitted-payout", "0", "0"]),
                    state(),
                ],
            ),
            (
                Family::SimpleSlot,
                vec![
                    c(
                        2,
                        &["1", "25", "reel", "reel", "reel", "flag", "omitted-payout"],
                    ),
                    state(),
                ],
            ),
        ] {
            let p = p(family);
            assert!(next_command(&p, &items).unwrap().is_none());
            closed(&p, &items).unwrap();
        }
    }
    #[test]
    fn keno_uses_two_known_numbers_and_twenty_distinct_native_balls() {
        let p = p(Family::Keno);
        let mut feature = vec![
            "0".into(),
            "metadata".into(),
            "omitted-payout".into(),
            "20".into(),
        ];
        feature.extend((1..=20).map(|n| n.to_string()));
        let items = vec![
            c(1, &["25", "ignored", "2", "1", "2"]),
            Command {
                code: 2,
                args: feature,
            },
        ];
        assert_eq!(next_command(&p, &items).unwrap().as_deref(), Some("3"));
        let mut all = items;
        all.extend([c(3, &["0", "0", "omitted-payout", "0"]), state()]);
        closed(&p, &all).unwrap();
        unknown(next_command(&p, &[c(2, &["unknown"])]).unwrap_err());
    }
    #[test]
    fn unreviewed_family_state_and_converter_never_default_to_an_action() {
        unknown(profile(Family::Poker, "wrong source", &initial(Family::Poker)).unwrap_err());
        unknown(next_command(&p(Family::Wheel), &[c(5, &["new-choice"])]).unwrap_err());
        unknown(closed(&p(Family::SimpleSlot), &[state()]).unwrap_err());
        let game = Game {
            id: "239".into(),
            name: "Keno".into(),
            modes: vec![],
        };
        let mode = Mode {
            key: "new-table-bet".into(),
            kind: ModeKind::Purchase,
            label: String::new(),
            requires_prerequisites: Value::Null,
        };
        assert!(!supports(&game, &mode));
    }
}
