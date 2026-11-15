//! Native capture protocol from standalone info/netent/codigo/netent_geco.rs.
//! Published evidence SHA-256: 1196bc9998ea68483348862008725bfe0385ea01e2bf62b0daacbb7da7beb7d1
//! Reference transport game-runner/src/adapters/netent_geco.rs; no runtime dependency.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode, ModeKind},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::Url;

#[derive(serde::Deserialize)]
struct ReviewedGame {
    table_id: String,
    game_id: String,
}

#[derive(serde::Deserialize)]
struct SpecialProfile {
    #[serde(flatten)]
    game: ReviewedGame,
    source_path: String,
    source_sha256: String,
    free_actions: Vec<String>,
    modes: BTreeMap<String, SpecialMode>,
}

#[derive(serde::Deserialize)]
struct SpecialMode {
    action: String,
    params: BTreeMap<String, String>,
}

fn special(game: &str, mode: &str) -> Option<(&'static SpecialProfile, &'static SpecialMode)> {
    static PROFILES: std::sync::OnceLock<BTreeMap<String, SpecialProfile>> =
        std::sync::OnceLock::new();
    let profile = PROFILES
        .get_or_init(|| {
            serde_json::from_slice(include_bytes!("../../bindings/netent-geco-modes.json"))
                .expect("reviewed NetEnt GECO special bindings")
        })
        .get(game)?;
    Some((profile, profile.modes.get(mode)?))
}

fn reviewed(game: &str) -> Option<&'static ReviewedGame> {
    static GAMES: std::sync::OnceLock<BTreeMap<String, ReviewedGame>> = std::sync::OnceLock::new();
    GAMES
        .get_or_init(|| {
            serde_json::from_slice(include_bytes!("../../bindings/netent-geco.json"))
                .expect("reviewed NetEnt GECO bindings")
        })
        .get(game)
}

pub fn supports(game: &Game, mode: &Mode) -> bool {
    (mode.kind == ModeKind::Normal && mode.key == "Spin" && reviewed(&game.id).is_some())
        || (matches!(
            mode.kind,
            ModeKind::Purchase | ModeKind::Variant | ModeKind::AnteBet | ModeKind::Booster
        ) && special(&game.id, &mode.key).is_some())
}

fn action_url(
    init: &Url,
    action: &str,
    wager: Option<&Quote>,
    mode: Option<(&SpecialProfile, &SpecialMode)>,
) -> Result<Url> {
    if !(matches!(
        (action, wager),
        ("spin", Some(_)) | ("drop" | "freespin" | "respin" | "initfreespin", None)
    ) || mode.is_some_and(|(profile, entry)| match wager {
        Some(_) => action == "featurebuy" && entry.action == action,
        None => {
            !matches!(action, "spin" | "featurebuy")
                && profile.free_actions.iter().any(|allowed| allowed == action)
        }
    })) {
        return Err(crate::model::unknown_event(
            "unreviewed GECO next action".to_string(),
        ));
    };
    let mut url = init.clone();
    url.set_query(None);
    let mut params = url.query_pairs_mut();
    params.extend_pairs(init.query_pairs().filter(|(key, _)| {
        matches!(
            key.as_ref(),
            "sessid" | "gameId" | "wantsfreerounds" | "freeroundmode"
        ) || key.starts_with("ne_")
    }));
    params
        .append_pair("action", action)
        .append_pair("wantsreels", "true")
        .append_pair("no-cache", &uuid::Uuid::new_v4().to_string());
    if let Some(wager) = wager {
        let mut quoted: BTreeMap<String, String> = wager
            .parameters()
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect();
        if let Some((_, entry)) = mode {
            quoted.extend(entry.params.clone());
        }
        params.extend_pairs(quoted);
    }
    drop(params);
    Ok(url)
}

pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    _settings: &Value,
) -> Result<Completion> {
    let selected = special(&game.id, &mode.key);
    let (binding, asset) = if let Some((profile, _)) = selected {
        (
            &profile.game,
            Some((profile.source_path.as_str(), profile.source_sha256.as_str())),
        )
    } else {
        (reviewed(&game.id).context("unreviewed GECO game")?, None)
    };
    let (init, referer) =
        super::netent_launch::open_geco(ctx, &game.id, &binding.table_id, &binding.game_id, asset)
            .await?;
    let native_headers = |referer: &str| {
        BTreeMap::from([
            ("accept".into(), "*/*".into()),
            ("referer".into(), referer.into()),
            ("cookie".into(), String::new()),
        ])
    };
    let response = ctx
        .http(
            "netent_geco_init",
            "GET",
            init.as_str(),
            native_headers(&referer),
            String::new(),
            false,
        )
        .await?;
    ensure!(
        response.status == 200,
        "HTTP {} at GECO initialization",
        response.status
    );
    let mut initial = fields(&response.body)?;
    ensure!(
        initial.get("gameId").is_none_or(|v| *v == binding.game_id),
        "GECO initialization owner changed"
    );
    initial
        .entry("gameId".into())
        .or_insert_with(|| binding.game_id.clone());
    let wager = quote(&initial)?;
    ctx.save_session(
        &json!({"geco_init":init.as_str(),"native_game":binding.game_id,"table":binding.table_id}),
    )?;
    let mut next = selected
        .map_or("spin", |(_, entry)| entry.action.as_str())
        .to_owned();
    for index in 0..ctx.max_steps().min(400) {
        let url = action_url(&init, &next, (index == 0).then_some(&wager), selected)?;
        let response = ctx
            .http(
                match (index, &mode.kind) {
                    (0, ModeKind::Purchase) => "netent_geco_purchase",
                    (0, _) => "netent_geco_spin",
                    _ => "netent_geco_continue",
                },
                "GET",
                url.as_str(),
                native_headers("https://showcase.evo-games.com/"),
                String::new(),
                true,
            )
            .await?;
        ensure!(
            response.status == 200,
            "HTTP {} at GECO action",
            response.status
        );
        let current = fields(&response.body)?;
        ensure!(
            required(&current, "playforfun")? == "true"
                && current.get("gameId").is_none_or(|v| *v == binding.game_id)
                && ["error", "errorcode"]
                    .into_iter()
                    .all(|k| current.get(k).is_none_or(|v| v.is_empty() || v == "0")),
            "GECO did not acknowledge the selected demo action"
        );
        next = required(&current, "nextaction")?.into();
        if required(&current, "gameover")? == "true" {
            if let Some((_, entry)) = selected
                && let Some(math) = entry.params.get("bet.mathMode")
            {
                ensure!(
                    current.get("mathMode") == Some(math),
                    "GECO math mode was not acknowledged"
                );
            }
            if next != "spin" {
                return Err(crate::model::unknown_event(format!(
                    "GECO terminal returned next action {next}"
                )));
            }
            let round = current
                .get("gameRoundId")
                .or_else(|| current.get("roundid"))
                .cloned();
            let mut outcome = Completion {
                native_round_id: round,
                details: json!({"native_game":binding.game_id,"table":binding.table_id}),
            };
            if let Some((profile, entry)) = selected {
                outcome.details["selector"] = json!({"mode":mode.key,"params":entry.params,"source_sha256":profile.source_sha256});
            }
            return Ok(outcome);
        }
    }
    Err(crate::model::unknown_event(
        "GECO continuation limit reached",
    ))
}

type Fields = BTreeMap<String, String>;

pub(crate) fn fields(data: &str) -> Result<Fields> {
    let mut fields = Fields::new();
    for (key, value) in url::form_urlencoded::parse(data.as_bytes()) {
        if let Some(previous) = fields.insert(key.into_owned(), value.to_string()) {
            ensure!(previous == value, "GECO response has conflicting fields");
        }
    }
    if !fields.contains_key("nextaction")
        && fields.get("gameover").is_some_and(|v| v == "true")
        && fields.get("allowedactions").is_some_and(|v| v == "spin")
    {
        fields.insert("nextaction".into(), "spin".into());
    }
    Ok(fields)
}

fn required<'a>(fields: &'a Fields, key: &str) -> Result<&'a str> {
    fields
        .get(key)
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .with_context(|| format!("GECO field {key} missing"))
}

fn idle(fields: &Fields) -> Result<()> {
    ensure!(
        required(fields, "playforfun")? == "true"
            && required(fields, "gameover")? == "true"
            && required(fields, "nextaction")? == "spin"
            && ["error", "errorcode"]
                .into_iter()
                .all(|k| fields.get(k).is_none_or(|v| v.is_empty() || v == "0")),
        "GECO response does not confirm an idle demo"
    );
    Ok(())
}

fn minimum(fields: &Fields, key: &str) -> Result<u64> {
    let mut min = u64::MAX;
    for entry in required(fields, key)?.split(',') {
        let value: u64 = entry
            .parse()
            .with_context(|| format!("invalid GECO menu {key}"))?;
        ensure!(value > 0, "GECO menu has a nonpositive option");
        min = min.min(value);
    }
    Ok(min)
}

#[derive(Debug, PartialEq)]
pub(crate) struct Quote {
    level: u64,
    denomination: u64,
    lines: String,
    cents: u64,
}

impl Quote {
    pub(crate) fn parameters(&self) -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            ("bet.betlevel", self.level.to_string()),
            ("bet.denomination", self.denomination.to_string()),
            ("bet.betlines", self.lines.clone()),
        ])
    }
}

pub(crate) fn quote(fields: &Fields) -> Result<Quote> {
    idle(fields)?;
    let level = minimum(fields, "betlevel.all")?;
    let denomination = minimum(fields, "denomination.all")?;
    let selection = required(fields, "bl.standard")?;
    let ids: Vec<_> = selection.split(',').collect();
    let (mut lines, mut weights) = if ids.len() == 1 && ids[0] != "0" {
        if !(required(fields, "bl.i0.line")?.contains('/')) {
            return Err(crate::model::unknown_event(
                "unreviewed GECO line selector".to_string(),
            ));
        };
        let mut ways = Vec::new();
        for key in fields.keys() {
            let Some(index) = key.strip_prefix("bl.i").and_then(|v| v.strip_suffix(".id")) else {
                continue;
            };
            let _: u64 = index.parse()?;
            let id: u64 = required(fields, key)?.parse()?;
            let coins: u64 = required(fields, &format!("bl.i{index}.coins"))?.parse()?;
            ensure!(
                coins != 0 && !ways.iter().any(|&(_, previous)| previous == id),
                "invalid GECO ways offer"
            );
            ways.push((coins, id));
        }
        let offered: u64 = selection.parse()?;
        ensure!(
            ways.iter().any(|&(_, id)| id == offered),
            "GECO selected ways absent from menu"
        );
        let (coins, id) = ways.into_iter().min().context("GECO ways menu empty")?;
        (id.to_string(), coins)
    } else {
        let mut indices = ids
            .iter()
            .map(|id| id.parse::<usize>())
            .collect::<std::result::Result<Vec<_>, _>>()?;
        indices.sort_unstable();
        if !(indices.into_iter().eq(0..ids.len())) {
            return Err(crate::model::unknown_event(
                "unreviewed GECO line indexing".to_string(),
            ));
        };
        let coins = (0..ids.len()).try_fold(0_u64, |sum, index| {
            let coins = required(fields, &format!("bl.i{index}.coins"))?.parse::<u64>()?;
            sum.checked_add(coins).context("GECO line cost overflow")
        })?;
        (format!("0-{}", ids.len() - 1), coins)
    };
    // These desktop clients send a title-specific selector for an unchanged wager.
    match (
        fields.get("gameId").map(String::as_str),
        lines.as_str(),
        weights,
    ) {
        (Some("fairymirror_not_mobile_sw"), "0-0", 10) => lines = "243".into(),
        (Some("narcos_not_mobile_sw"), "243", 20) => lines = "0-0".into(),
        // Reviewed config SHA-256: 92dc14c248bd857c349f29f4883eea852b83e97ff6be3ed02843644d4d67c932.
        (Some("butterflystaxx2_not_mobile_sw"), "0-39", 40) => weights = 25,
        (
            Some(
                "fairymirror_not_mobile_sw"
                | "narcos_not_mobile_sw"
                | "butterflystaxx2_not_mobile_sw",
            ),
            _,
            _,
        ) => {
            anyhow::bail!("reviewed GECO client has an unexpected line offer")
        }
        _ => {}
    }
    let cents = denomination
        .checked_mul(level)
        .and_then(|n| n.checked_mul(weights))
        .filter(|v| *v > 0)
        .context("invalid GECO base cost")?;
    Ok(Quote {
        denomination,
        level,
        lines,
        cents,
    })
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn request_quote_uses_no_credit_and_continuation_never_rebuys() {
        let initial=fields("playforfun=true&gameover=true&nextaction=spin&betlevel.all=1&denomination.all=1&bl.standard=0&bl.i0.coins=20").unwrap();
        let wager = quote(&initial).unwrap();
        assert_eq!(wager.parameters()["bet.betlevel"], "1");
        let init = Url::parse(
            "https://showcase.evo-games.com/native?sessid=owner&gameId=fixture&ne_mode=demo",
        )
        .unwrap();
        let paid = action_url(&init, "spin", Some(&wager), None).unwrap();
        assert!(paid.query_pairs().any(|(key, _)| key == "bet.betlevel"));
        let continued = action_url(&init, "drop", None, None).unwrap();
        assert!(
            !continued
                .query_pairs()
                .any(|(key, _)| key.starts_with("bet."))
        );
        assert!(
            action_url(&init, "novel_action", None, None)
                .unwrap_err()
                .downcast_ref::<crate::model::UnknownEvent>()
                .is_some()
        );
    }
}
