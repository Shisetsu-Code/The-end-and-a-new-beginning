//! Source-backed RPC families ported from the archived BGaming Python adapters.
//! Native fields are preserved. Wallet/payout reconciliation is intentionally absent.
use super::*;
fn contracts() -> &'static Value {
    static B: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    B.get_or_init(|| {
        serde_json::from_str(include_str!("../../bindings/bgaming-rpc-contracts.json"))
            .expect("BGaming RPC contracts")
    })
}
pub(super) fn family(game: &str, mode: &Mode) -> Option<&'static str> {
    let g = &legacy_extra::bindings()["games"][game];
    if g["modes"][&mode.key] == mode.kind.as_str() {
        g["family"].as_str().filter(|s| s.starts_with("rpc:"))
    } else {
        None
    }
}
fn scripts(html: &str) -> Result<Vec<String>> {
    require_source(
        !Regex::new(r"(?i)<base\b")?.is_match(html),
        "native script base",
    )?;
    let mut result = Vec::new();
    let local_script = Regex::new(
        r"^(?:\./|/)?[A-Za-z0-9_./-]+\.js(?:\?(?:[a-f0-9]{8,64}|(?:v|ver|version|hash)=[A-Za-z0-9_.-]+))?$",
    )?;
    for c in Regex::new(r#"<script[^>]+src=["']([^"']+)["']"#)?.captures_iter(html) {
        let route = &c[1];
        if local_script.is_match(route) && !route.split('/').any(|p| p == "..") {
            result.push(format!(
                "/{}",
                route.trim_start_matches("./").trim_start_matches('/')
            ));
        }
    }
    Ok(result)
}
async fn script_source(ctx: &RunContext, page: &Url, html: &str, family: &str) -> Result<String> {
    let entries = scripts(html)?;
    let pattern = match family {
        "recycle-riches" | "blazing-firepots" => r"^/main\.js(?:\?.*)?$",
        "blackbeards-bounty" => r"^/assets/index-[A-Za-z0-9_-]+\.js(?:\?.*)?$",
        "jungle-queen" | "zeus-goes-wild" => r"^/bundle\.js(?:\?.*)?$",
        "sugar-mix" | "treasure-explorer" => r"^/main\.[a-f0-9]+\.js(?:\?.*)?$",
        _ => r".*",
    };
    let re = Regex::new(pattern)?;
    let chosen: Vec<_> = entries.iter().filter(|v| re.is_match(v)).collect();
    require_source(chosen.len() == 1, "unique native RPC entry")?;
    let mut source = get(ctx, "bgaming_rpc_source", page.join(chosen[0])?.as_str()).await?;
    if chosen[0].split('?').next() == Some("/loader.js") {
        let re = Regex::new(r#"res:[A-Za-z_$][\w$]*="([A-Za-z0-9_.-]+)""#)?;
        let version = re
            .captures(&source)
            .ok_or_else(|| unknown_event("BGaming versioned RPC loader unresolved"))?;
        require_source(
            html.contains("resources_path:\".\"")
                && html.contains("game_bundle_source:\"bundle.js\""),
            "native loader resource defaults",
        )?;
        source = get(
            ctx,
            "bgaming_rpc_versioned_source",
            page.join(&format!("/{}/bundle.js", &version[1]))?.as_str(),
        )
        .await?;
    }
    Ok(source)
}
fn feature_allowed(initial: &Value, feature: &Value) -> Result<()> {
    require_source(
        initial["config"]["purchased_features"]
            .as_array()
            .is_some_and(|a| a.contains(feature)),
        "active native RPC product",
    )
}
struct Plan {
    family: String,
    entry: Value,
    bet: Value,
    meta: Value,
    mode: String,
}
#[derive(Default)]
struct State {
    gambled: bool,
    seen: BTreeSet<(usize, usize)>,
    signature: String,
    choice_seen: bool,
    identifier_policy: Option<bool>,
    action_ids: BTreeSet<String>,
}
fn extra_plan(initial: &Value, mode: &Mode, source: &str, family: &str) -> Result<Plan> {
    let descriptor = &contracts()["families"][family];
    let tokens = descriptor["required_tokens"]
        .as_array()
        .ok_or_else(|| unknown_event("BGaming RPC source contract missing"))?;
    require_source(
        tokens
            .iter()
            .all(|v| v.as_str().is_some_and(|t| source.contains(t))),
        "complete game-specific RPC source contract",
    )?;
    let bet = json!(minimum(&initial["config"]["bet_limits"])?);
    let mut request = json!({"bet":bet});
    let mut product = None;
    if mode.key != "normal" {
        product = descriptor["products"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["id"] == mode.key);
        require_source(product.is_some(), "source-defined RPC selector")?;
    }
    if let Some(p) = product {
        if !p["feature"].is_null() {
            feature_allowed(initial, &p["feature"])?;
        }
        request.as_object_mut().unwrap().extend(
            p["request"]
                .as_object()
                .ok_or_else(|| unknown_event("BGaming RPC request fields missing"))?
                .clone(),
        );
    } else {
        match family {
            "blackbeards-bounty" => request["bet_type"] = json!("bet"),
            "blazing-firepots" => {
                request["bet_type"] = json!("betting");
                request["action"] = json!("spin");
            }
            "jungle-queen" => {
                request["bet_type"] = json!("bet");
                request["action"] = json!("spin");
            }
            "sugar-mix" => {
                request["bet_type"] = json!("default");
                request["purchased_feature"] = Value::Null;
            }
            "treasure-explorer" => request["machineId"] = json!(6),
            _ => {}
        }
    }
    if matches!(family, "keepers-secret" | "grand-patron") {
        request["bet"] = json!(bet.to_string());
    }
    if family == "grand-patron" && request.get("bid").is_none() {
        request["bid"] = json!(0);
    }
    if family == "zeus-goes-wild" {
        request["bet_type"] = json!("default");
        request["custom_field"] = json!("custom_value");
    }
    if matches!(family, "zeus-goes-wild" | "clash-gods") {
        request["fe_exponent"] = json!(precision(initial)?);
    }
    Ok(Plan {
        family: family.into(),
        entry: request,
        bet,
        meta: json!({}),
        mode: mode.key.clone(),
    })
}
fn precision(initial: &Value) -> Result<u64> {
    let bets = initial["config"]["bet_limits"]
        .as_array()
        .ok_or_else(|| unknown_event("BGaming RPC bet list missing"))?;
    let mut exponent = 2;
    for bet in bets {
        let n = bet
            .as_u64()
            .ok_or_else(|| unknown_event("BGaming integer native bet missing"))?;
        let rest = n % 100;
        if rest != 0 {
            exponent = exponent.max(if rest % 10 == 0 { 2 } else { 3 });
        }
    }
    Ok(exponent)
}
fn product_plan(initial: &Value, mode: &Mode, source: &str) -> Result<Plan> {
    let wires:BTreeSet<String>=Regex::new(r"const [A-Za-z_$][\w$]*=\{bet:[A-Za-z_$][\w$]*,purchased_feature:[A-Za-z_$][\w$]*,(buy_id|bonus_buy):[A-Za-z_$][\w$]*\}")?.captures_iter(source).map(|c|c[1].into()).collect();
    require_source(
        wires.len() == 1
            && (source.contains("this._getPlayResponse(null)")
                || source.contains("this._getPlayResponse()")),
        "product ID wire and automatic continuation",
    )?;
    let mut arrays = Vec::new();
    for m in Regex::new(r"function [A-Za-z_$][\w$]*\(\)\{return\[")?.find_iter(source) {
        let a = source::balanced(source, m.end() - 1)?;
        if a.contains("purchaseFeature:") && a.contains("price:") {
            arrays.push(a);
        }
    }
    require_source(arrays.len() == 1, "unique source product registry")?;
    let bet = json!(minimum(&initial["config"]["bet_limits"])?);
    let mut entry = json!({"bet":bet});
    let array = arrays[0];
    let mut found = mode.key == "normal";
    let mut pos = 1;
    while pos < array.len() - 1 {
        while array
            .as_bytes()
            .get(pos)
            .is_some_and(|c| c.is_ascii_whitespace() || *c == b',')
        {
            pos += 1;
        }
        if pos >= array.len() - 1 {
            break;
        }
        let obj = source::balanced(array, pos)?;
        pos += obj.len();
        let fields = source::fields(obj)?;
        let id = source::literal(
            source,
            fields
                .get("id")
                .ok_or_else(|| unknown_event("BGaming native product ID missing"))?,
        )?;
        if id == mode.key {
            let feature = source::literal(
                source,
                fields
                    .get("purchaseFeature")
                    .ok_or_else(|| unknown_event("BGaming native product feature missing"))?,
            )?;
            feature_allowed(initial, &feature)?;
            entry[wires.first().unwrap()] = id;
            entry["purchased_feature"] = feature;
            found = true;
        }
    }
    require_source(found, "requested native product registry entry")?;
    Ok(Plan {
        family: "product".into(),
        entry,
        bet,
        meta: json!({}),
        mode: mode.key.clone(),
    })
}
fn classic_plan(
    initial: &Value,
    mode: &Mode,
    source: &str,
    integration: &str,
    client: &str,
    defs: &Value,
    star: bool,
) -> Result<Plan> {
    require_source(
        client.contains("params.req.custom_req=")
            && client.contains("formattedRequest.params.action="),
        "classic OGA envelope",
    )?;
    let re = Regex::new(r"\.Regular\](?:=|:)\{ActionType:[^{}]*AdditionalData:\{[^{}]*params:\{")?;
    let (text, m) = if let Some(m) = re.find_iter(source).last() {
        (source, m)
    } else {
        (
            integration,
            re.find_iter(integration)
                .last()
                .ok_or_else(|| unknown_event("BGaming OGA regular params absent"))?,
        )
    };
    let fields = source::fields(source::balanced(text, m.end() - 1)?)?;
    let mut params = json!({});
    let array_length = Regex::new(r"Array\.from\(\{length:(\d+)")?;
    for (key, value) in fields {
        if key == "selectedWinLines" && value.starts_with("Array.from(") {
            let n = array_length
                .captures(&value)
                .and_then(|c| c[1].parse::<usize>().ok())
                .filter(|v| *v > 0 && *v < 1000)
                .ok_or_else(|| unknown_event("BGaming consecutive line array unproved"))?;
            params[&key] = json!((0..n).collect::<Vec<_>>());
        } else {
            params[&key] = source::literal(text, &value)?;
        }
    }
    let mut key = mode.key.as_str();
    let mut choice = 1;
    if let Some((base, value)) = key.split_once("/choice:") {
        key = base;
        choice = value.parse::<u64>()?;
        require_source(
            star && (1..=4).contains(&choice),
            "source Star choice domain",
        )?;
    }
    if key != "normal" {
        let flag = match key {
            "normalBuyCost" => "isNormalBuy",
            "superBuyCost" => "isSuperBuy",
            "featureBuyMulFreespin" => "isFeatureBuyFreeSpin",
            "featureBuyMulRespin" => "isFeatureBuyRespin",
            _ => return Err(unknown_event("BGaming OGA product not source mapped")),
        };
        require_source(
            !defs["engine"]["definition"][key].is_null()
                && params.get(flag).is_some()
                && (source.contains(&format!(".params.{flag}=!0"))
                    || Regex::new(&format!(
                        r#"\.params\.{}="(?:FreeSpin|Respin)"==="#,
                        regex::escape(flag)
                    ))?
                    .is_match(source)),
            "source OGA purchase flag",
        )?;
        feature_allowed(initial, &json!("buy_bonus"))?;
        params[flag] = json!(true);
    }
    let bet = json!(minimum(&initial["config"]["bet_limits"])?);
    params["action"] = json!("spin");
    params["exponent"] = json!(2);
    if client.contains("formattedRequest.params.stake=") {
        params["stake"] = bet.clone();
    }
    let mut entry = json!({"bet":bet,"bet_type":"bet","custom_req":params});
    if key != "normal" {
        entry["purchased_feature"] = json!("buy_bonus");
    }
    if star {
        for token in [
            ".BONUS=\"fs_choice\"",
            ".PICK_RESPIN=\"pick_respin\"",
            ".CASH_PRIZE=\"cash_prize\"",
            "this.onPickMeObjectClicked(e+1)",
            "[this.lowVolatility,this.mediumVolatility,this.highVolatility,this.gamble]",
        ] {
            require_source(source.contains(token), "Star choice source")?;
        }
    }
    if star {
        require_source(
            Regex::new(r"\.params\.choice=([A-Za-z_$][\w$]*)\.BGInput\.Object")?.is_match(source),
            "Star one-based choice wire",
        )?;
        let methods: Vec<_> = Regex::new(r"defineNextRound\([^)]*\)\{")?
            .find_iter(integration)
            .collect();
        let method = methods
            .last()
            .ok_or_else(|| unknown_event("BGaming Star transition source missing"))?;
        let body = source::balanced(integration, method.end() - 1)?;
        for token in [
            "scatterPositions.length",
            "numFreeSpins",
            "numOfRespin",
            "revealOrder",
            "isCashPrizeAwarded",
        ] {
            require_source(body.contains(token), "Star source transition predicates")?;
        }
    }
    let mut combined = json!({});
    let re = Regex::new(
        r"\.AdditionalData\.request=([A-Za-z_$][\w$]*)\.(FREESPIN_RESPIN|JACKPOT_RESPIN|FREESPIN_JACKPOT|FREESPIN_JACKPOT_RESPIN)",
    )?;
    for c in re.captures_iter(source) {
        let alias = Regex::new(&format!(
            r#"{}\.{}="([a-z_]+)""#,
            regex::escape(&c[1]),
            regex::escape(&c[2])
        ))?;
        if let Some(a) = alias.captures(source) {
            combined[&a[1]] = json!({"action":&a[1]});
        }
    }
    Ok(Plan {
        family: if star { "star" } else { "classic" }.into(),
        entry,
        bet,
        meta: json!({"choice":choice,"combined":combined,"send_custom_stake":client.contains("formattedRequest.params.stake=")}),
        mode: mode.key.clone(),
    })
}
fn ordinary_plan(initial: &Value, mode: &Mode, source: &str) -> Result<Plan> {
    let bet = json!(minimum(&initial["config"]["bet_limits"])?);
    let key = mode.key.split('/').next().unwrap();
    let mut entry = json!({"bet":bet,"bet_type":"bet","custom_req":{"exponent":2}});
    if key != "normal" {
        let mut visible = BTreeSet::new();
        let ante_member = Regex::new(r"ante:[A-Za-z_$][\w$]*\.([\w$]+)")?;
        let feature_member = Regex::new(r"[A-Za-z_$][\w$]*\.([\w$]+)")?;
        for c in
            Regex::new(r"getPurchasedFeatureConfig\(\)\{return\{([^}]+)\}")?.captures_iter(source)
        {
            for c in ante_member.captures_iter(&c[1]) {
                visible.insert(c[1].to_owned());
            }
        }
        for c in Regex::new(r"getBuyFeaturePurchasedValues\(\)\{return\[([^\]]+)\]")?
            .captures_iter(source)
        {
            for c in feature_member.captures_iter(&c[1]) {
                visible.insert(c[1].to_owned());
            }
        }
        require_source(visible.contains(key), "source-visible RPC feature")?;
        feature_allowed(initial, &json!(key))?;
        entry["purchased_feature"] = json!(key);
    }
    let mut choices = json!({});
    for c in
        Regex::new(r"params:\{choice:([A-Za-z_$][\w$]*)\.(Collect|Gamble)\}")?.captures_iter(source)
    {
        let re = Regex::new(&format!(r"\.{}=(\d+)", regex::escape(&c[2])))?;
        let start = source
            .find(&format!("var {};", &c[1]))
            .ok_or_else(|| unknown_event("BGaming risk enum declaration missing"))?;
        let end = source[start..]
            .find(&format!("}}({}||", &c[1]))
            .map(|v| v + start)
            .ok_or_else(|| unknown_event("BGaming risk enum body missing"))?;
        let id = re
            .captures(&source[start..end])
            .and_then(|c| c[1].parse::<u64>().ok())
            .ok_or_else(|| unknown_event("BGaming risk literal missing"))?;
        choices[c[2].to_lowercase()] = json!(id);
    }
    Ok(Plan {
        family: "ordinary".into(),
        entry,
        bet,
        meta: json!({"choices":choices}),
        mode: mode.key.clone(),
    })
}
fn boolean(v: &Value) -> Result<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) if n.as_u64() == Some(0) => Ok(false),
        Value::Number(n) if n.as_u64() == Some(1) => Ok(true),
        _ => Err(unknown_event("BGaming native lifecycle boolean missing")),
    }
}
fn final_pair_entry(
    initial: &Value,
    mode: &Mode,
    game: &str,
    last: &Value,
    first: bool,
) -> Result<Value> {
    let bet = minimum(&initial["config"]["bet_limits"])?;
    let feature = (mode.key != "normal").then_some(mode.key.as_str());
    if let Some(f) = feature {
        feature_allowed(initial, &json!(f))?;
    }
    if game == "YommiRush" {
        let mut req = json!({"bet":bet.to_string(),"bet_type":"bet","modelRev":0,"minExponent":2});
        if first && let Some(f) = feature {
            req["purchased_feature"] = json!(f);
        }
        return Ok(req);
    }
    let mut req = json!({"bet":bet,"bet_type":"default","fe_exponent":precision(initial)?,"purchased_feature":if first||feature==Some("buy_chance"){json!(feature)}else{Value::Null},"buyBonusModeMultiplier":if first&&feature==Some("buy_bonus"){60}else{1}});
    if let Some(balance) = last.get("balance") {
        req["balance"] = balance.clone();
    }
    Ok(req)
}
fn star_next(result: &Value, choice: &Value) -> Result<String> {
    let g = &result["resp"]["engine"]["gamestate"];
    let current = g["action"]
        .as_str()
        .ok_or_else(|| unknown_event("BGaming Star native action missing"))?;
    let fs = &g["freespinState"];
    let rs = &g["respinState"];
    let pick = &g["pickState"];
    let cash = &rs["cashPrize"];
    let reveal = &rs["revealOrder"];
    let respins = rs["numOfRespin"].as_i64().unwrap_or(0);
    let freespins = fs["numFreeSpins"].as_i64().unwrap_or(0);
    let total = reveal["total_pick"].as_i64().unwrap_or(0);
    let picked = pick["pick_index"].as_i64().unwrap_or(-1) + 1;
    let mut action = None;
    if respins > 0 {
        action = Some("respin");
    }
    if freespins > 0 {
        action = Some("freespin");
    }
    if matches!(current, "spin" | "autospin" | "pick_respin" | "cash_prize")
        && g["triggeringDetails"]["scatterPositions"]
            .as_array()
            .is_some_and(|a| a.len() == 3)
        && respins == 0
        && (current != "pick_respin" || total == picked || pick.is_null())
        && (current != "cash_prize" || cash["winnings"] != 0)
    {
        action = Some("fs_choice");
    }
    if current == "respin" && total > 0
        || current == "pick_respin" && !reveal.is_null() && picked != total
    {
        action = Some("pick_respin");
    }
    if current == "pick_respin" && picked == total && rs["isCashPrizeAwarded"] == true
        || current == "cash_prize"
            && cash["level"].as_i64().unwrap_or(0) > 0
            && cash["winnings"] == 0
            && rs["isCashPrizeAwarded"] == true
    {
        action = Some("cash_prize");
    }
    let _ = choice;
    action
        .map(str::to_owned)
        .ok_or_else(|| unknown_event("BGaming unknown Star transition"))
}
fn next(
    plan: &Plan,
    result: &Value,
    step: usize,
    state: &mut State,
    initial: &Value,
    game: &str,
) -> Result<Option<Value>> {
    if plan.family == "final" {
        if step == 0 && plan.mode != "normal" {
            if game == "YommiRush" {
                let model = match plan.mode.as_str() {
                    "buy_chance" => "MORE_PETS",
                    "buy_bonus" => "BONUS",
                    "buy_bonus_and_chance" => "SUPER_BONUS",
                    _ => return Err(unknown_event("BGaming unbound Yommi feature")),
                };
                require_source(
                    result["req"]["purchased_feature"] == plan.mode
                        || result["resp"]["state"]["boughtFeature"] == model,
                    "native Yommi purchased feature identity",
                )?;
            } else if plan.mode == "buy_bonus" {
                require_source(
                    boolean(&result["resp"]["super_spin"]["round"]["stages"][0]["isBuyBonus"])?,
                    "native Joker purchase confirmation",
                )?;
            }
        }
        let closed = if game == "YommiRush" {
            boolean(&result["resp"]["outcome"]["roundEnd"])?
        } else {
            let stages = result["resp"]["super_spin"]["round"]["stages"]
                .as_array()
                .filter(|a| a.len() == 1)
                .ok_or_else(|| unknown_event("BGaming Joker single native stage missing"))?;
            if step > 0 {
                require_source(
                    boolean(&stages[0]["isFreespin"])?,
                    "Joker free continuation",
                )?;
            }
            !boolean(&stages[0]["isNextFree"])?
        };
        if let Some(flag) = result.get("final") {
            require_source(
                boolean(flag)? == closed,
                "matching native final/source lifecycle",
            )?;
        }
        return if closed {
            Ok(None)
        } else {
            Ok(Some(final_pair_entry(
                initial,
                &Mode {
                    key: plan.mode.clone(),
                    kind: ModeKind::Normal,
                    label: String::new(),
                    requires_prerequisites: Value::Null,
                },
                game,
                result,
                false,
            )?))
        };
    }
    let final_flag = result["final"]
        .as_bool()
        .ok_or_else(|| unknown_event("BGaming RPC final flag missing"))?;
    if plan.family == "aztecs-claw" && step == 1 {
        require_source(
            result["resp"]["bonus_game"] == "WILD_DICES_FINISH",
            "Aztec explicit finish acknowledgment",
        )?;
        return Ok(Some(
            json!({"bet":plan.bet,"bet_type":"bet","bonus_game":"WILD_DICES_FINISH"}),
        ));
    }
    if final_flag {
        return Ok(None);
    }
    let mut req = json!({"bet":plan.bet});
    match plan.family.as_str() {
        "modern-cats" => {
            let raw = &result["resp"]["body"];
            let body = if let Some(s) = raw.as_str() {
                serde_json::from_str::<Value>(s)?
            } else {
                raw.clone()
            };
            let current = &body["currentBonus"];
            let next = &body["nextBonus"];
            let pending = next["bonusName"]
                .as_str()
                .is_some_and(|s| s.contains("free"))
                || current["bonusName"]
                    .as_str()
                    .is_some_and(|s| s.contains("free"))
                    && current["currentSpin"]
                        .as_i64()
                        .or_else(|| current["spinIndex"].as_i64())
                        .unwrap_or(0)
                        < current["totalSpins"]
                            .as_i64()
                            .or_else(|| current["totalSpinCount"].as_i64())
                            .unwrap_or(0);
            require_source(pending, "native Cats free-spin state")?;
            req["bet_type"] = json!("bet");
        }
        "modern-mystic" => req["bet_type"] = json!("bet"),
        "product" | "recycle-riches" | "big-bucks-saloon" => {}
        "blackbeards-bounty" => req["bet_type"] = json!("bet"),
        "blazing-firepots" => {
            let action = result["resp"]["nextAction"]
                .as_str()
                .filter(|v| matches!(*v, "SPIN" | "WHEEL" | "BONUS"))
                .ok_or_else(|| unknown_event("BGaming Firepots nextAction missing"))?;
            req["bet_type"] = json!("betting");
            req["action"] = json!(action.to_lowercase());
        }
        "jungle-queen" => {
            require_source(
                matches!(
                    result["resp"]["state"]["status"].as_str(),
                    Some("FREESPINS_TRIGGER" | "FREESPINS")
                ),
                "Jungle free continuation",
            )?;
            req["bet_type"] = json!("bet");
            req["action"] = json!("freespin");
        }
        "sugar-mix" => {
            req["bet_type"] = json!("default");
            req["purchased_feature"] = Value::Null;
        }
        "treasure-explorer" => req["machineId"] = json!(6),
        "keepers-secret" => req["bet"] = json!(plan.bet.to_string()),
        "grand-patron" => {
            req["bet"] = json!(plan.bet.to_string());
            req["bid"] = plan.entry["bid"].clone();
        }
        "zeus-goes-wild" => {
            req["bet_type"] = json!("default");
            req["custom_field"] = json!("custom_value");
            req["fe_exponent"] = plan.entry["fe_exponent"].clone();
        }
        "clash-gods" => {
            req["bet_type"] = json!("default");
            req["feature_buy"] = Value::Null;
            req["purchased_feature"] = Value::Null;
            req["buyBonusModeMultiplier"] = json!(1);
            req["bonus_type"] = json!(if plan.mode.ends_with("_anubis") {
                "1"
            } else {
                "0"
            });
            req["fe_exponent"] = plan.entry["fe_exponent"].clone();
        }
        "aztecs-claw" => {
            require_source(
                step == 0 && result["resp"]["bonus_game"] == "WILD_DICES",
                "Aztec native bonus trigger",
            )?;
            req["bet_type"] = json!("bet");
            req["bonus_game"] = json!("WILD_DICES");
        }
        "ordinary" | "classic" | "star" => {
            req["bet_type"] = json!("bet");
            let action = &result["resp"]["engine"]["gamestate"]["triggeringDetails"]["nextAction"];
            let mut custom = json!({"exponent":2});
            if plan.family == "star" {
                let a = star_next(result, &plan.meta["choice"])?;
                custom["action"] = json!(a);
                if a == "fs_choice" {
                    custom["choice"] = plan.meta["choice"].clone();
                    state.choice_seen = true;
                }
            } else if let Some(choices) = action.as_array() {
                require_source(
                    !choices.is_empty() && choices.iter().all(|v| v == "gamble" || v == "collect"),
                    "native RPC risk choices",
                )?;
                let choose = if plan.mode.ends_with("/gamble_once")
                    && !state.gambled
                    && choices.iter().any(|v| v == "gamble")
                {
                    state.gambled = true;
                    "gamble"
                } else {
                    "collect"
                };
                require_source(
                    choices.iter().any(|v| v == choose) && !plan.meta["choices"][choose].is_null(),
                    "source risk choice mapping",
                )?;
                custom["choice"] = plan.meta["choices"][choose].clone();
            } else {
                let a = action
                    .as_str()
                    .ok_or_else(|| unknown_event("BGaming RPC next action missing"))?;
                require_source(
                    matches!(
                        a,
                        "freespin"
                            | "respin"
                            | "jackpot"
                            | "bonus"
                            | "cascade"
                            | "spin"
                            | "cascadefg"
                            | "extra_bet_hold"
                    ) || !plan.meta["combined"][a].is_null(),
                    "native RPC continuation action",
                )?;
                if plan.family == "classic" {
                    custom["action"] = json!(a);
                }
            }
            if plan.family == "classic"
                && custom["action"] == "spin"
                && plan.meta["send_custom_stake"] == true
            {
                custom["stake"] = plan.bet.clone();
            }
            req["custom_req"] = custom;
        }
        "cocos" => return cocos_next(plan, result, state),
        _ => return Err(unknown_event("BGaming RPC family has no continuation")),
    }
    Ok(Some(req))
}
fn cocos_uuid(value: &str) -> Result<String> {
    if Regex::new(r"^[0-9a-fA-F-]{36}$")?.is_match(value) {
        return Ok(value.into());
    }
    require_source(
        value.len() == 22 && value.as_bytes()[..2].iter().all(u8::is_ascii_hexdigit),
        "native Cocos compressed UUID",
    )?;
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut raw = value[..2].to_owned();
    for pair in value.as_bytes()[2..].as_chunks::<2>().0 {
        let a = alphabet
            .find(pair[0] as char)
            .ok_or_else(|| unknown_event("BGaming Cocos UUID alphabet"))?;
        let b = alphabet
            .find(pair[1] as char)
            .ok_or_else(|| unknown_event("BGaming Cocos UUID alphabet"))?;
        raw.push_str(&format!("{:03x}", a * 64 + b));
    }
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &raw[..8],
        &raw[8..12],
        &raw[12..16],
        &raw[16..20],
        &raw[20..]
    ))
}
fn cocos_route(manifest: &Value, bundle: &str, key: &str) -> Result<String> {
    let matches: Vec<_> = manifest["paths"]
        .as_object()
        .ok_or_else(|| unknown_event("BGaming Cocos manifest paths missing"))?
        .iter()
        .filter(|(_, v)| v[0] == key)
        .collect();
    require_source(matches.len() == 1, "unique Cocos config asset")?;
    let index: usize = matches[0].0.parse()?;
    let uid = cocos_uuid(
        text(&manifest["uuids"], &index.to_string())
            .unwrap_or_else(|_| manifest["uuids"][index].as_str().unwrap_or("")),
    )?;
    let base = text(manifest, "importBase")?;
    let path_token = Regex::new(r"^[A-Za-z0-9_-]+$")?;
    require_source(path_token.is_match(base), "Cocos import base")?;
    for pack in manifest["packs"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
    {
        require_source(
            !pack
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == index)),
            "unpacked Cocos config asset",
        )?;
    }
    let mut suffix = String::new();
    for pair in manifest["versions"]["import"]
        .as_array()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .as_chunks::<2>()
        .0
    {
        if *pair[0] == index {
            let version = pair[1]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| pair[1].to_string());
            require_source(path_token.is_match(&version), "Cocos config version")?;
            suffix = format!(".{version}");
        }
    }
    Ok(format!(
        "/assets/{bundle}/{base}/{}/{uid}{suffix}.json",
        &uid[..2]
    ))
}
fn find_config(value: &Value, predicate: fn(&Value) -> bool, out: &mut Vec<Value>) {
    if predicate(value) {
        out.push(value.clone());
    }
    match value {
        Value::Object(m) => {
            for v in m.values() {
                find_config(v, predicate, out)
            }
        }
        Value::Array(a) => {
            for v in a {
                find_config(v, predicate, out)
            }
        }
        _ => {}
    }
}
fn merge(to: &mut Value, from: &Value) {
    if let (Some(a), Some(b)) = (to.as_object_mut(), from.as_object()) {
        for (k, v) in b {
            if let Some(old) = a.get_mut(k) {
                merge(old, v)
            } else {
                a.insert(k.clone(), v.clone());
            }
        }
    } else {
        *to = from.clone();
    }
}
async fn cocos_plan(ctx: &RunContext, page: &Url, initial: &Value, mode: &Mode) -> Result<Plan> {
    let source = get(
        ctx,
        "bgaming_cocos_source",
        page.join("/assets/main/index.js")?.as_str(),
    )
    .await?;
    for token in [
        "RequestBuilder",
        "buildBuyBonusRequest",
        "custom_field:{fid:",
    ] {
        require_source(source.contains(token), "Cocos RequestBuilder source")?;
    }
    let manifest: Value = serde_json::from_str(
        &get(
            ctx,
            "bgaming_cocos_manifest",
            page.join("/assets/config-base/config.json")?.as_str(),
        )
        .await?,
    )?;
    let route = cocos_route(&manifest, "config-base", "base.config")?;
    let asset: Value = serde_json::from_str(
        &get(ctx, "bgaming_cocos_config", page.join(&route)?.as_str()).await?,
    )?;
    let mut configs = Vec::new();
    find_config(
        &asset,
        |v| {
            v["features"].as_array().is_some_and(|a| {
                a.iter()
                    .all(|v| v.get("id").is_some() && v.get("priceMultiplier").is_some())
            })
        },
        &mut configs,
    );
    require_source(configs.len() == 1, "unique Cocos base config")?;
    let mut config = configs.remove(0);
    if source.contains(".loadConfig(\"prod\")") {
        let manifest: Value = serde_json::from_str(
            &get(
                ctx,
                "bgaming_cocos_prod_manifest",
                page.join("/assets/config-prod/config.json")?.as_str(),
            )
            .await?,
        )?;
        let route = cocos_route(&manifest, "config-prod", "prod.config")?;
        let asset: Value = serde_json::from_str(
            &get(
                ctx,
                "bgaming_cocos_prod_config",
                page.join(&route)?.as_str(),
            )
            .await?,
        )?;
        let mut candidates = Vec::new();
        find_config(
            &asset,
            |v| {
                ["api", "project", "features", "logger", "env"]
                    .iter()
                    .any(|k| v.get(k).is_some())
            },
            &mut candidates,
        );
        if let Some(override_config) = candidates.first() {
            merge(&mut config, override_config);
        }
    }
    let aliases = Regex::new(r#""BUY_BONUS_PRODUCT_PURCHASED_FEATURES",\{([^}]+)\}"#)?
        .captures(&source)
        .ok_or_else(|| unknown_event("BGaming Cocos feature aliases missing"))?;
    let ids = Regex::new(r#""BUY_BONUS_PRODUCT_FEATURE_IDS",\{([^}]+)\}"#)?
        .captures(&source)
        .ok_or_else(|| unknown_event("BGaming Cocos product IDs missing"))?;
    let pairs = Regex::new(r#"(\w+):"([^"]+)""#)?;
    let purchased: BTreeMap<String, String> = pairs
        .captures_iter(&aliases[1])
        .map(|c| (c[1].into(), c[2].into()))
        .collect();
    let mut feature_ids: BTreeMap<String, String> = pairs
        .captures_iter(&ids[1])
        .map(|c| (c[1].into(), c[2].into()))
        .collect();
    if let Some(c) = Regex::new(r#""CHANCE_FEATURE_ID","([^"]+)""#)?.captures(&source) {
        feature_ids.insert("chance_x2".into(), c[1].into());
    }
    let mut bids: BTreeMap<String, u64> = Regex::new(r"(\w+):\{bid:(\d+),purchasedFeature:")?
        .captures_iter(&source)
        .filter_map(|c| c[2].parse().ok().map(|v| (c[1].into(), v)))
        .collect();
    if let Some(c) = Regex::new(r"\{(\d+):[A-Za-z_$][\w$]*\.chance_x2\}")?.captures(&source) {
        bids.insert("chance_x2".into(), c[1].parse()?);
    }
    let bet = json!(minimum(&initial["config"]["bet_limits"])?);
    let key = mode.key.split('/').next().unwrap();
    let mut entry = json!({"bet":bet,"bet_type":"bet","bid":0});
    if key != "normal" {
        let id = feature_ids
            .get(key)
            .ok_or_else(|| unknown_event("BGaming Cocos requested product alias missing"))?;
        require_source(
            config["features"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v["id"] == *id)),
            "native Cocos configured product",
        )?;
        let feature = purchased
            .get(key)
            .ok_or_else(|| unknown_event("BGaming Cocos purchased feature alias missing"))?;
        entry["purchased_feature"] = json!(feature);
        entry["bid"] = json!(
            bids.get(key)
                .ok_or_else(|| unknown_event("BGaming Cocos bid missing"))?
        );
    }
    let board = Regex::new(r#""BoardSymbol",\{([^}]+)\}"#)?
        .captures(&source)
        .map(|c| c[1].to_owned())
        .unwrap_or_default();
    let empty = Regex::new(r"(?:^|,)Empty:(\d+)")?
        .captures(&board)
        .and_then(|c| c[1].parse::<u64>().ok());
    let risk = source.contains(r#"fid:{a:"collect"==="#) && source.contains(r#"?"no":"yes""#);
    Ok(Plan {
        family: "cocos".into(),
        entry,
        bet,
        meta: json!({"empty":empty,"risk":risk}),
        mode: mode.key.clone(),
    })
}
fn cocos_next(plan: &Plan, result: &Value, state: &mut State) -> Result<Option<Value>> {
    let resp = &result["resp"];
    let step = &resp["step"];
    let expected = step["er"]
        .as_array()
        .ok_or_else(|| unknown_event("BGaming Cocos expected actions missing"))?;
    let mut req = json!({"bet":plan.bet,"bet_type":"bet"});
    if expected.iter().any(|v| {
        v["esmid"]
            .as_str()
            .is_some_and(|s| s.to_uppercase().contains("RISK"))
    }) {
        require_source(plan.meta["risk"] == true, "Cocos source risk wire")?;
        let gamble = plan.mode.ends_with("/gamble_once") && !state.gambled;
        state.gambled |= gamble;
        req["custom_field"] = json!({"fid":{"a":if gamble{"yes"}else{"no"}}});
        return Ok(Some(req));
    }
    require_source(
        !expected.is_empty()
            && expected.iter().all(|v| {
                v["esmid"].as_str().is_some_and(|s| {
                    s.to_uppercase().starts_with("FS1_") || s.to_uppercase().starts_with("FS2_")
                })
            })
            && !plan.meta["empty"].is_null(),
        "Cocos source hidden-cell phase",
    )?;
    let layout = if step["ff"][0]["f"].is_array() {
        &step["ff"][0]["f"]
    } else {
        &step["f"]
    };
    let signature = json!([expected, layout]).to_string();
    if state.signature != signature {
        state.seen.clear();
        state.signature = signature;
    }
    let mut known = vec![step["fid"].clone()];
    let raw = &step["cd"];
    let cd = if let Some(s) = raw.as_str() {
        serde_json::from_str::<Value>(s)?
    } else {
        raw.clone()
    };
    known.extend(cd["fe"].as_array().into_iter().flatten().cloned());
    for cell in known {
        if let (Some(x), Some(y)) = (cell["x"].as_u64(), cell["y"].as_u64()) {
            state.seen.insert((x as usize, y as usize));
        }
    }
    let columns = layout
        .as_array()
        .ok_or_else(|| unknown_event("BGaming Cocos native board missing"))?;
    for (x, column) in columns.iter().enumerate() {
        let cells = column
            .as_array()
            .ok_or_else(|| unknown_event("BGaming Cocos native column malformed"))?;
        for (y, cell) in cells.iter().enumerate() {
            if cell["e"] == plan.meta["empty"] && state.seen.insert((x, y)) {
                req["bid"] = resp["round"]["bet"]["id"].clone();
                require_source(!req["bid"].is_null(), "native Cocos round bid")?;
                req["custom_field"] = json!({"fid":{"x":x,"y":y}});
                return Ok(Some(req));
            }
        }
    }
    Err(unknown_event(
        "BGaming no source-verified unopened Cocos cell",
    ))
}
async fn modern_plan(
    ctx: &RunContext,
    page: &Url,
    html: &str,
    initial: &Value,
    mode: &Mode,
    game: &str,
) -> Result<Plan> {
    let pins = contracts()["modern_pins"][game]
        .as_object()
        .ok_or_else(|| unknown_event("BGaming modern source pins missing"))?;
    let entries = scripts(html)?;
    let mut sources = BTreeMap::new();
    for (route, hash) in pins {
        if route.ends_with(".js") {
            require_source(
                entries
                    .iter()
                    .any(|v| v.split('?').next() == Some(route.as_str())),
                "HTML-selected pinned native script",
            )?;
        }
        let source = get(ctx, "bgaming_modern_source", page.join(route)?.as_str()).await?;
        require_source(
            sha256(source.as_bytes()) == hash.as_str().unwrap_or(""),
            "pinned modern native client",
        )?;
        sources.insert(route.clone(), source);
    }
    let bet = json!(minimum(&initial["config"]["bet_limits"])?);
    let mut entry = json!({"bet":bet,"bet_type":"bet"});
    let family = if game == "CatsLoveYummy" {
        let cfg: Value = serde_json::from_str(&sources["/slot_assets/gameConfig.json"])?;
        require_source(
            cfg["featureOnOff"]["buyFeature"] == true,
            "Cats native buy visibility",
        )?;
        if mode.key != "normal" {
            let configs = cfg["buyFeatureInfo"]["configs"]
                .as_array()
                .filter(|v| v.len() == 1)
                .ok_or_else(|| unknown_event("BGaming Cats product registry changed"))?;
            require_source(
                mode.key == "buy_feature:1"
                    && configs[0]["buyFeatureId"] == 1
                    && configs[0]["purchasedFeature"] == "buy_bonus",
                "Cats native buy ID",
            )?;
            feature_allowed(initial, &json!("buy_bonus"))?;
            entry["purchased_feature"] = json!("buy_bonus");
            entry["buy_feature_id"] = json!(1);
        }
        "modern-cats"
    } else {
        let lookup: Value = serde_json::from_str(&sources["/lookup_table_1.0.18.json"])?;
        let route = format!(
            "/{}",
            text(&lookup["table"], "config/config.json")?.trim_start_matches('/')
        );
        let cfg: Value = serde_json::from_str(
            sources
                .get(&route)
                .ok_or_else(|| unknown_event("BGaming Mystic source config route changed"))?,
        )?;
        let controllers: Vec<_> = cfg["controllers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["type"] == "GameModeController")
            .collect();
        require_source(controllers.len() == 1, "unique Mystic mode controller")?;
        let name = match mode.key.as_str() {
            "normal" => "BASIC",
            "ENCORE" => "ENCORE",
            "BONUSBUY1" => "BONUSBUY1",
            _ => return Err(unknown_event("BGaming unknown Mystic mode")),
        };
        require_source(
            controllers[0]["modes"]
                .as_array()
                .is_some_and(|a| a.iter().any(|m| m["name"] == name)),
            "native Mystic product visibility",
        )?;
        if mode.key != "normal" {
            let feature = if mode.key == "ENCORE" {
                "buy_chance"
            } else {
                "buy_bonus"
            };
            feature_allowed(initial, &json!(feature))?;
            entry["purchased_feature"] = json!(feature);
        }
        "modern-mystic"
    };
    Ok(Plan {
        family: family.into(),
        entry,
        bet,
        meta: json!({}),
        mode: mode.key.clone(),
    })
}
async fn prepare(
    ctx: &RunContext,
    page: &Url,
    html: &str,
    initial: &Value,
    mode: &Mode,
    game: &str,
    family: &str,
) -> Result<Plan> {
    let family = family.strip_prefix("rpc:").unwrap_or(family);
    if family == "modern" {
        return modern_plan(ctx, page, html, initial, mode, game).await;
    }
    if family == "cocos" || html.contains("src/import-map.json") && html.contains("System.import") {
        return cocos_plan(ctx, page, initial, mode).await;
    }
    if family == "final" {
        let pin = if game == "YommiRush" {
            "fde03472331ff66c19c1566000073e641f325cf1e47735eb8d7742bc9f5f2b83"
        } else {
            "f1502a31115a48f4f347bf74ffcdd92798f69bb9e56df290b351d4dd2894ec5d"
        };
        let mut found = false;
        for route in scripts(html)? {
            let source = get(ctx, "bgaming_final_source", page.join(&route)?.as_str()).await?;
            if sha256(source.as_bytes()) == pin {
                found = true;
                break;
            }
        }
        require_source(found, "exact final-pair native source")?;
        return Ok(Plan {
            family: "final".into(),
            entry: final_pair_entry(initial, mode, game, initial, true)?,
            bet: json!(minimum(&initial["config"]["bet_limits"])?),
            meta: json!({}),
            mode: mode.key.clone(),
        });
    }
    if family != "oga" && family != "star" {
        let source = script_source(ctx, page, html, family).await?;
        if family == "product" || family == "auto" {
            return product_plan(initial, mode, &source);
        }
        return extra_plan(initial, mode, &source, family);
    }
    let defs: Value = serde_json::from_str(
        &get(
            ctx,
            "bgaming_oga_definitions",
            page.join("/game/media/definitions.json")?.as_str(),
        )
        .await?,
    )?;
    let source = get(
        ctx,
        "bgaming_oga_source",
        page.join("/game/game.min.js")?.as_str(),
    )
    .await?;
    let client = get(
        ctx,
        "bgaming_oga_client",
        page.join("/client.min.js")?.as_str(),
    )
    .await?;
    if client.contains("formattedRequest.params.action=") {
        let integration = get(
            ctx,
            "bgaming_oga_integration",
            page.join("/game/integration.min.js")?.as_str(),
        )
        .await?;
        classic_plan(
            initial,
            mode,
            &source,
            &integration,
            &client,
            &defs,
            family == "star",
        )
    } else {
        ordinary_plan(initial, mode, &source)
    }
}
fn validate_target(plan: &Plan, state: &State) -> Result<()> {
    if plan.mode.contains("/choice:") && !state.choice_seen {
        bail!("BGaming target_not_observed: requested Star free-spin choice was not offered");
    }
    if plan.mode.ends_with("/gamble_once") && !state.gambled {
        bail!("BGaming target_not_observed: requested risk branch was not offered");
    }
    Ok(())
}

fn yommi_correlation(result: &Value, last: &Value, state: &mut State) -> Result<()> {
    let round_present = result.get("round").is_some();
    let step_present = result.get("step").is_some();
    require_source(
        round_present == step_present,
        "Yommi complete round/step presence",
    )?;
    require_source(
        state
            .identifier_policy
            .is_none_or(|known| known == round_present),
        "stable Yommi identifier policy",
    )?;
    if round_present {
        require_source(
            native_id(&result["round"]).is_some(),
            "nonempty Yommi round",
        )?;
        let step = native_id(&result["step"])
            .ok_or_else(|| unknown_event("BGaming native Yommi action ID missing"))?;
        require_source(state.action_ids.insert(step), "unique Yommi native step")?;
    } else {
        // rpc_result already checked owned request/reply ID equality. With both
        // native IDs absent, each side of the sequential state lock is mandatory.
        require_source(
            last["state_lock"].as_str().is_some_and(|s| !s.is_empty())
                && result["state_lock"].as_str().is_some_and(|s| !s.is_empty()),
            "Yommi owned request/reply state locks",
        )?;
    }
    state.identifier_policy = Some(round_present);
    Ok(())
}

pub(super) async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    page: Url,
    config: Value,
    family: &str,
) -> Result<Completion> {
    require_source(
        page.path() == "/hyperhive"
            && page
                .host_str()
                .is_some_and(|h| h.ends_with(".demo.bgaming-network.com")),
        "native RPC public demo origin",
    )?;
    let token = text(&config, "play_token")?;
    let mut frontend = page.join("/")?;
    frontend.query_pairs_mut().append_pair("token", token);
    let html = get(ctx, "bgaming_rpc_frontend", frontend.as_str()).await?;
    let endpoint = page.join("/api")?;
    let h = headers(&page);
    let id = Uuid::new_v4().to_string();
    let mut params = json!({"token":token});
    if html.contains("src/import-map.json") && html.contains("System.import") {
        params["req"] = json!({});
    }
    let initial = rpc_result(
        ctx.json(
            "bgaming_rpc_init",
            endpoint.as_str(),
            &json!({"id":id,"jsonrpc":"2.0","method":"init","params":params}),
            h.clone(),
            false,
        )
        .await?,
        &id,
    )?;
    require_source(
        initial["state"].is_null() && initial["final"] != false,
        "fresh RPC state",
    )?;
    let plan = prepare(ctx, &page, &html, &initial, mode, &game.id, family).await?;
    ctx.save_session(&json!({"provider":"bgaming","game":game.id,"token":token,"endpoint":endpoint.as_str(),"family":plan.family}))?;
    let mut request = plan.entry.clone();
    let mut last = initial.clone();
    let mut state = State::default();
    let mut round = None;
    for step in 0..ctx.max_steps().min(512) {
        let id = Uuid::new_v4().to_string();
        let mut params = json!({"token":token,"req":request});
        if game.id != "JokerVsJoker" {
            params["state_lock"] = last["state_lock"].as_str().unwrap_or("").into();
        }
        let result = rpc_result(
            ctx.json(
                "bgaming_rpc_play",
                endpoint.as_str(),
                &json!({"id":id,"jsonrpc":"2.0","method":"play","params":params}),
                h.clone(),
                true,
            )
            .await?,
            &id,
        )?;
        if game.id == "YommiRush" {
            yommi_correlation(&result, &last, &mut state)?;
        }
        if let Some(id) = native_id(&result["round"]) {
            require_source(
                round.as_ref().is_none_or(|v| v == &id),
                "native RPC round identity",
            )?;
            round.get_or_insert(id);
        }
        let Some(next) = next(&plan, &result, step, &mut state, &initial, &game.id)? else {
            validate_target(&plan, &state)?;
            return Ok(Completion {
                native_round_id: round,
                details: json!({"family":plan.family,"mode":mode.key,"steps":step+1}),
            });
        };
        request = next;
        last = result;
    }
    Err(unknown_event("BGaming RPC continuation budget exhausted"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mode(key: &str) -> Mode {
        Mode {
            key: key.into(),
            kind: ModeKind::Purchase,
            label: String::new(),
            requires_prerequisites: Value::Null,
        }
    }
    #[test]
    fn product_registry_drives_id_and_free_continuation_never_rebuys() {
        let source = r#"const q={bet:b,purchased_feature:f,buy_id:i};function products(){return[{id:"boost",price:5,purchaseFeature:"buy_chance",activation:!0}]}this._getPlayResponse(null)"#;
        let initial = json!({"config":{"bet_limits":[20,10],"purchased_features":["buy_chance"]}});
        let p = product_plan(&initial, &mode("boost"), source).unwrap();
        assert_eq!(
            p.entry,
            json!({"bet":10,"buy_id":"boost","purchased_feature":"buy_chance"})
        );
        assert_eq!(
            next(
                &p,
                &json!({"final":false,"resp":{"round":{}}}),
                0,
                &mut State::default(),
                &initial,
                "MultiRush"
            )
            .unwrap()
            .unwrap(),
            json!({"bet":10})
        );
        assert!(
            next(
                &p,
                &json!({"final":true}),
                1,
                &mut State::default(),
                &initial,
                "MultiRush"
            )
            .unwrap()
            .is_none()
        );
    }
    #[test]
    fn rpc_final_without_wallet_or_payout_closes_and_unknown_action_is_typed() {
        let p = Plan {
            family: "ordinary".into(),
            entry: json!({}),
            bet: json!(10),
            meta: json!({}),
            mode: "normal".into(),
        };
        assert!(
            next(
                &p,
                &json!({"final":true}),
                0,
                &mut State::default(),
                &json!({}),
                "HotRocketMegaPotz"
            )
            .unwrap()
            .is_none()
        );
        let e=next(&p,&json!({"final":false,"resp":{"engine":{"gamestate":{"triggeringDetails":{"nextAction":"new_action"}}}}}),0,&mut State::default(),&json!({}),"HotRocketMegaPotz").unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn cocos_risk_wire_and_hidden_cells_are_source_bound_and_not_repeated() {
        let p = Plan {
            family: "cocos".into(),
            entry: json!({}),
            bet: json!(10),
            meta: json!({"empty":0,"risk":true}),
            mode: "coin_hunt/gamble_once".into(),
        };
        let mut s = State::default();
        let r = json!({"resp":{"step":{"er":[{"esmid":"RISK"}]}}});
        assert_eq!(
            cocos_next(&p, &r, &mut s).unwrap().unwrap()["custom_field"],
            json!({"fid":{"a":"yes"}})
        );
        assert_eq!(
            cocos_next(&p, &r, &mut s).unwrap().unwrap()["custom_field"],
            json!({"fid":{"a":"no"}})
        );
        let r = json!({"resp":{"round":{"bet":{"id":2}},"step":{"er":[{"esmid":"FS1_PICK"}],"f":[[{"e":0},{"e":0}]],"cd":{}}}});
        let first = cocos_next(&p, &r, &mut s).unwrap().unwrap();
        let second = cocos_next(&p, &r, &mut s).unwrap().unwrap();
        assert_ne!(first, second);
        assert!(cocos_next(&p, &r, &mut s).is_err());
    }
    #[test]
    fn source_final_pair_supports_native_lifecycle_without_balance() {
        let p = Plan {
            family: "final".into(),
            entry: json!({}),
            bet: json!(20),
            meta: json!({}),
            mode: "normal".into(),
        };
        assert!(
            next(
                &p,
                &json!({"resp":{"outcome":{"roundEnd":true}},"final":true}),
                0,
                &mut State::default(),
                &json!({}),
                "YommiRush"
            )
            .unwrap()
            .is_none()
        );
        let e = next(
            &p,
            &json!({"resp":{"outcome":{"roundEnd":true}},"final":false}),
            0,
            &mut State::default(),
            &json!({}),
            "YommiRush",
        )
        .unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
    #[test]
    fn classic_oga_native_flags_lines_and_unknown_products_are_not_guessed() {
        let initial = json!({"config":{"bet_limits":[20,10],"purchased_features":["buy_bonus"]}});
        let source = r#"x.Regular]={ActionType:1,AdditionalData:{params:{selectedWinLines:Array.from({length:3},(_,i)=>i),isNormalBuy:!1,isSuperBuy:!1}}};x.params.isNormalBuy=!0;x.params.isSuperBuy=!0;"#;
        let client =
            "params.req.custom_req=;formattedRequest.params.action=;formattedRequest.params.stake=";
        let defs = json!({"engine":{"definition":{"normalBuyCost":100,"superBuyCost":300}}});
        let p = classic_plan(
            &initial,
            &mode("normalBuyCost"),
            source,
            "",
            client,
            &defs,
            false,
        )
        .unwrap();
        assert_eq!(
            p.entry["custom_req"],
            json!({"selectedWinLines":[0,1,2],"isNormalBuy":true,"isSuperBuy":false,"action":"spin","exponent":2,"stake":10})
        );
        assert!(
            classic_plan(
                &initial,
                &mode("inventedBuy"),
                source,
                "",
                client,
                &defs,
                false
            )
            .is_err()
        );
        let continuation=next(&p,&json!({"final":false,"resp":{"engine":{"gamestate":{"triggeringDetails":{"nextAction":"freespin"}}}}}),0,&mut State::default(),&initial,"TheGodfather3PillarsOfPower").unwrap().unwrap();
        assert_eq!(
            continuation,
            json!({"bet":10,"bet_type":"bet","custom_req":{"action":"freespin","exponent":2}})
        );
    }
    #[test]
    fn grand_patron_normal_keeps_native_bid_zero_across_steps() {
        let source = contracts()["families"]["grand-patron"]["required_tokens"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>()
            .join(";");
        let initial = json!({"config":{"bet_limits":[10]}});
        let p = extra_plan(&initial, &mode("normal"), &source, "grand-patron").unwrap();
        assert_eq!(p.entry, json!({"bet":"10","bid":0}));
        assert_eq!(
            next(
                &p,
                &json!({"final":false}),
                0,
                &mut State::default(),
                &initial,
                "GrandPatron7rst"
            )
            .unwrap()
            .unwrap(),
            json!({"bet":"10","bid":0})
        );
        let g = Game {
            id: "GrandPatron7rst".into(),
            name: String::new(),
            modes: vec![],
        };
        assert!(!supports(&g, &mode("normal")));
    }
    #[test]
    fn oga_spin_continuation_keeps_source_stake_but_other_actions_omit_it() {
        let p = Plan {
            family: "classic".into(),
            entry: json!({}),
            bet: json!(10),
            meta: json!({"send_custom_stake":true}),
            mode: "normal".into(),
        };
        let response = |action| json!({"final":false,"resp":{"engine":{"gamestate":{"triggeringDetails":{"nextAction":action}}}}});
        let spin = next(
            &p,
            &response("spin"),
            0,
            &mut State::default(),
            &json!({}),
            "TheGodfather3PillarsOfPower",
        )
        .unwrap()
        .unwrap();
        assert_eq!(spin["custom_req"]["stake"], 10);
        let free = next(
            &p,
            &response("freespin"),
            1,
            &mut State::default(),
            &json!({}),
            "TheGodfather3PillarsOfPower",
        )
        .unwrap()
        .unwrap();
        assert!(free["custom_req"].get("stake").is_none());
    }
    #[test]
    fn yommi_absent_ids_require_owned_locks_and_policy_cannot_change() {
        let last = json!({"state_lock":"owned-before"});
        let mut state = State::default();
        assert!(
            yommi_correlation(
                &json!({"resp":{"outcome":{"roundEnd":false}}}),
                &last,
                &mut state
            )
            .is_err()
        );
        assert!(
            yommi_correlation(&json!({"round":"r","state_lock":"next"}), &last, &mut state)
                .is_err()
        );
        yommi_correlation(&json!({"state_lock":"next"}), &last, &mut state).unwrap();
        assert!(
            yommi_correlation(
                &json!({"round":"r","step":"1","state_lock":"next2"}),
                &last,
                &mut state
            )
            .is_err()
        );
        let mut state = State::default();
        yommi_correlation(&json!({"round":"r","step":"1"}), &json!({}), &mut state).unwrap();
        assert!(
            yommi_correlation(&json!({"round":"r","step":"1"}), &json!({}), &mut state).is_err()
        );
        assert!(yommi_correlation(&json!({"state_lock":"next"}), &last, &mut state).is_err());
    }
    #[test]
    fn star_explicit_choice_requires_observed_request_and_source_mapping() {
        let p = Plan {
            family: "star".into(),
            entry: json!({}),
            bet: json!(10),
            meta: json!({"choice":3}),
            mode: "featureBuyMulFreespin/choice:3".into(),
        };
        let mut state = State::default();
        assert!(
            next(
                &p,
                &json!({"final":true}),
                0,
                &mut state,
                &json!({}),
                "StarTrekNextGen"
            )
            .unwrap()
            .is_none()
        );
        let e = validate_target(&p, &state).unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_none());
        let r = json!({"final":false,"resp":{"engine":{"gamestate":{"action":"spin","triggeringDetails":{"scatterPositions":[1,2,3]},"freespinState":{},"respinState":{}}}}});
        let q = next(&p, &r, 0, &mut state, &json!({}), "StarTrekNextGen")
            .unwrap()
            .unwrap();
        assert_eq!(
            q["custom_req"],
            json!({"action":"fs_choice","exponent":2,"choice":3})
        );
        assert!(validate_target(&p, &state).is_ok());
        let initial = json!({"config":{"bet_limits":[10]}});
        let source = r#"x.Regular]={ActionType:1,AdditionalData:{params:{isFeatureBuyFreeSpin:!1}}};.BONUS="fs_choice";.PICK_RESPIN="pick_respin";.CASH_PRIZE="cash_prize";this.onPickMeObjectClicked(e+1);[this.lowVolatility,this.mediumVolatility,this.highVolatility,this.gamble]"#;
        let client = "params.req.custom_req=;formattedRequest.params.action=";
        let integration = "defineNextRound(x){scatterPositions.length;numFreeSpins;numOfRespin;revealOrder;isCashPrizeAwarded;}";
        assert!(
            classic_plan(
                &initial,
                &mode("normal"),
                source,
                integration,
                client,
                &json!({}),
                true
            )
            .is_err()
        );
        let source = format!("{source};x.params.choice=E.BGInput.Object;");
        assert!(
            classic_plan(
                &initial,
                &mode("normal"),
                &source,
                integration,
                client,
                &json!({}),
                true
            )
            .is_ok()
        );
        assert!(
            classic_plan(
                &initial,
                &mode("normal"),
                &source,
                &integration.replace("revealOrder", "unknown"),
                client,
                &json!({}),
                true
            )
            .is_err()
        );
    }
}
