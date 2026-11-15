//! Remaining source-backed legacy families from native_variants.py, old_slots.py,
//! classic_slots.py, cats_choices.py, alice_choices.py and bonus_choices_extra.py.
use super::*;

pub(super) fn bindings() -> &'static Value {
    static B: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    B.get_or_init(|| {
        serde_json::from_str(include_str!("../../bindings/bgaming-extra.json"))
            .expect("BGaming extra bindings")
    })
}
pub(super) fn family(game: &str, mode: &Mode) -> Option<&'static str> {
    let g = &bindings()["games"][game];
    if g["modes"][&mode.key] == mode.kind.as_str() {
        g["family"].as_str()
    } else {
        None
    }
}
fn allowed(reply: &Value, command: &str, v1: bool) -> bool {
    let values = if v1 {
        &reply["available_commands"]
    } else {
        &reply["flow"]["available_actions"]
    };
    values
        .as_array()
        .is_some_and(|a| a.iter().any(|v| v == command))
}
fn source_v1(source: &str, game: &str, classic: bool) -> Result<()> {
    if classic {
        require_source(
            Regex::new(&format!(r"\.games\.{}=\{{", regex::escape(game)))?.is_match(source)
                && Regex::new(r"for\(let ([A-Za-z_$][\w$]*) in ([A-Za-z_$][\w$]*)\.config\.bet\)")?
                    .is_match(source)
                && source.contains(".config.subunits)")
                && source.contains("command:\"finish\"")
                && source.contains(".command=\"spin\""),
            "classic indexed bets and finish",
        )?;
    } else {
        require_source(
            Regex::new(r#"\.isApi2="2"===[A-Za-z_$][\w$]*\.api_version"#)?.is_match(source)
                && source.contains(".options.lines.length")
                && source.contains(".isApi2?\"close\":\"finish\""),
            "v1 line dictionary/finish",
        )?;
    }
    Ok(())
}
fn free_spin_wire(source: &str) -> Result<bool> {
    let branches = Regex::new(r"if\([A-Za-z_$][\w$]*\.isApi2\)(.*?);else\{")?;
    let dispatch = Regex::new(r#"([A-Za-z_$][\w$]*)&&\([A-Za-z_$][\w$]*\.command="freespin"\)"#)?;
    let bets_assignment = Regex::new(r"\.bets=[A-Za-z_$][\w$]*")?;
    for branch in branches.captures_iter(source) {
        if let Some(flag) = dispatch.captures(&branch[1]) {
            let body = source::balanced(source, branch.get(0).unwrap().end() - 1)?;
            let flag_used = Regex::new(&format!(
                r"(?:^|[^\w$]){}(?:[^\w$]|$)",
                regex::escape(&flag[1])
            ))?
            .is_match(body);
            if !flag_used && bets_assignment.is_match(body) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn v1_entry(initial: &Value, mode: &Mode, source: &str) -> Result<Value> {
    let line = minimum(&initial["options"]["line_bets"])?;
    let lines = initial["options"]["lines"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| unknown_event("BGaming native v1 lines missing"))?;
    let mut bets = serde_json::Map::new();
    for i in 0..lines.len() {
        bets.insert(i.to_string(), json!(line));
    }
    let mut entry = json!({"bets":bets});
    if mode.key == "feature_buy" {
        require_source(
            initial["options"]["buy_feature_value"]
                .as_i64()
                .is_some_and(|v| v > 0)
                && Regex::new(r"[A-Za-z_$][\w$]*&&\([A-Za-z_$][\w$]*\.buy_feature=!0\)")?
                    .is_match(source),
            "native v1 feature flag",
        )?;
        entry["buy_feature"] = json!(true);
    }
    Ok(entry)
}
#[derive(Default)]
struct Choices {
    gambled: bool,
    used: BTreeSet<usize>,
    steps: usize,
    extras: BTreeSet<String>,
    target_observed: bool,
}
struct Plan {
    entry: Value,
    base: Option<LegacyPlan>,
    card_count: usize,
    policy: String,
    first_door: Option<usize>,
    target_extra: Option<String>,
}
fn plan(initial: &Value, mode: &Mode, source: &str, game: &str, family: &str) -> Result<Plan> {
    let mut card_count = 0;
    let mut policy = String::new();
    let mut first_door = None;
    let mut target_extra = None;
    let mut key = mode.key.clone();
    if family == "variants" {
        return Ok(Plan {
            entry: source::variant_entry(initial, mode, source)?,
            base: None,
            card_count,
            policy,
            first_door,
            target_extra,
        });
    }
    if family == "v1" {
        return Ok(Plan {
            entry: v1_entry(initial, mode, source)?,
            base: None,
            card_count,
            policy: mode.key.clone(),
            first_door,
            target_extra,
        });
    }
    if family == "alice" {
        for token in [
            r#"{command:"gamble_bonus",options:{}}"#,
            r#"this.requestCardsPick({mode:"select_pick_cards"})"#,
            r#"this.requestCardsPick({mode:"auto"})"#,
            r#"this.requestCardsPick({mode:"any",index:"#,
            "get index(){return this.parent.parent.children.indexOf(this.parent)}",
        ] {
            require_source(source.contains(token), "Alice choice request")?;
        }
        let containers: Vec<_> = Regex::new(r#"\{c:"MovieClip",p:\{name:"cards-list""#)?
            .find_iter(source)
            .collect();
        require_source(containers.len() == 1, "unique Alice card container")?;
        let body = source::balanced(source, containers[0].start())?;
        card_count = body
            .matches("prefabName:\"slot-animations/popup/pick-feature-card\"")
            .count();
        require_source(
            (2..=64).contains(&card_count)
                && Regex::new(r#"image:"popups/features-to-pick/card-(?:green|red)-\d+\.png""#)?
                    .find_iter(body)
                    .count()
                    == card_count,
            "Alice source card positions",
        )?;
        let parts: Vec<_> = key.split('/').collect();
        if parts.len() > 1 {
            require_source(
                parts.len() == 3
                    && matches!(parts[1], "collect" | "gamble_once")
                    && matches!(parts[2], "auto" | "manual_low" | "manual_high"),
                "Alice explicit policy",
            )?;
            policy = format!("{}/{}", parts[1], parts[2]);
            key = parts[0].into();
        } else {
            policy = "implicit:collect/auto".into();
        }
    }
    if family == "cats" {
        require_source(
            source.contains(r#"{command:"play_bonus",options:{index:i}}"#)
                && source.contains(".parent.parent.children.indexOf("),
            "Cats bottle index wire",
        )?;
        let containers: Vec<_> = Regex::new(r#"\{c:"MovieClip",p:\{name:"bottles""#)?
            .find_iter(source)
            .collect();
        require_source(containers.len() == 1, "unique Cats bottle container")?;
        let fields = source::fields(source::balanced(source, containers[0].start())?)?;
        let children = fields
            .get(":")
            .ok_or_else(|| unknown_event("BGaming Cats bottle children missing"))?;
        let mut pos = 1;
        while pos < children.len() - 1 {
            while children
                .as_bytes()
                .get(pos)
                .is_some_and(|c| c.is_ascii_whitespace() || *c == b',')
            {
                pos += 1;
            }
            if pos >= children.len() - 1 {
                break;
            }
            let child = source::balanced(children, pos)?;
            require_source(
                child.starts_with("{c:\"MovieClip\""),
                "Cats bottle child type",
            )?;
            card_count += 1;
            pos += child.len();
        }
        require_source(card_count == 6, "six native Cats bottles")?;
    }
    if family == "wildmoon" {
        require_source(
            sha256(source.as_bytes())
                == "9bec7ce8437c85e5d4a234f865065574c1165b7556b96ef0b0bd5928790f0f0e",
            "Wild Moon source version",
        )?;
        if let Some(rest) = key.strip_prefix("manual_first_door:") {
            let (value, base) = rest
                .split_once('/')
                .ok_or_else(|| unknown_event("BGaming manual door mode missing trigger"))?;
            let index: usize = value.parse()?;
            require_source(
                index < 16 && source.contains(&format!("clickDoor`{index}")),
                "native manual door",
            )?;
            first_door = Some(index);
            key = base.into();
        }
        require_source(
            source.contains(r#"this.sendCommand({index:t})"#)
                && source.contains(r#"this.sendCommand({mode:"auto"})"#),
            "Wild Moon choice envelope",
        )?;
    }
    if family == "extras" {
        let (base, extra) = key
            .split_once("/extra-first:")
            .ok_or_else(|| unknown_event("BGaming extra mode target missing"))?;
        let (target, _attempt) = extra
            .split_once("/attempt:")
            .ok_or_else(|| unknown_event("BGaming extra attempt missing"))?;
        require_source(
            matches!(
                target,
                "extra_spin" | "extra_board" | "extra_multiplier" | "regenerate_bonus"
            ) && source.contains(&format!(r#"{target}:{{command:"{target}""#))
                && Regex::new(
                    r#"\{command:"buy_extra_bonus",options:\{bonus_type:[A-Za-z_$][\w$]*\}\}"#,
                )?
                .is_match(source),
            "native optional extra purchase",
        )?;
        target_extra = Some(target.into());
        key = base.into();
    }
    if family == "dig" {
        require_source(
            sha256(source.as_bytes())
                == "fb184c286f5cf73a3f1bcaedc1b22dbbbb8aa1050dd08e649a5cf7971f04fa97",
            "Dig card source version",
        )?;
        policy = key
            .strip_prefix("conditional_card/")
            .ok_or_else(|| unknown_event("BGaming Dig semantic card missing"))?
            .into();
        require_source(
            matches!(policy.as_str(), "s" | "c" | "d" | "h" | "black" | "red"),
            "Dig semantic card",
        )?;
        key = "normal".into();
    }
    let m = Mode {
        key,
        kind: mode.kind,
        label: String::new(),
        requires_prerequisites: Value::Null,
    };
    let base = legacy_plan(initial, &m, source, game)?;
    Ok(Plan {
        entry: base.entry.clone(),
        base: Some(base),
        card_count,
        policy,
        first_door,
        target_extra,
    })
}
fn next(
    reply: &Value,
    plan: &Plan,
    state: &mut Choices,
    source: &str,
    game: &str,
    family: &str,
) -> Result<Option<(String, Option<Value>)>> {
    let v1 = family == "v1";
    let flow = if v1 { &reply["game"] } else { &reply["flow"] };
    let phase = flow["state"]
        .as_str()
        .ok_or_else(|| unknown_event("BGaming native phase missing"))?;
    if phase == "closed" {
        if family == "alice"
            && !plan.policy.starts_with("implicit:")
            && (state.steps == 0
                || plan.policy.starts_with("gamble_once/") && !state.gambled
                || plan.policy.contains("/manual_") && state.used.is_empty())
        {
            bail!(
                "BGaming target_not_observed: requested Alice card/gamble branch was not offered"
            );
        }
        if family == "wildmoon" && plan.first_door.is_some() && state.steps == 0 {
            bail!("BGaming target_not_observed: requested manual door branch was not offered");
        }
        if (family == "dig" || plan.policy.starts_with("semantic_card:")) && !state.gambled
            || plan.target_extra.is_some() && !state.target_observed
        {
            bail!(
                "BGaming target_not_observed: requested conditional choice was not offered in this round"
            );
        }
        return Ok(None);
    }
    let action: (String, Option<Value>) = if v1 {
        if phase == "check_card" && plan.policy.starts_with("semantic_card:") && !state.gambled {
            let choice = plan.policy.trim_start_matches("semantic_card:");
            require_source(
                matches!(choice, "s" | "c" | "d" | "h" | "black" | "red")
                    && sha256(source.as_bytes())
                        == "bbfca3c458135f414aaf3c441f015ab330a5d410f2d99b1654ee270efce899ed",
                "classic semantic card source",
            )?;
            state.gambled = true;
            ("check_card".into(), Some(json!({"bet":choice})))
        } else if phase == "freespin" && allowed(reply, "spin", true) {
            require_source(
                free_spin_wire(source)?
                    && reply["game"]["freespins_left"]
                        .as_u64()
                        .is_some_and(|n| n > 0),
                "source-proven v1 free-spin dispatch with positive native counter",
            )?;
            let bets = &reply["bets"]["lines"];
            require_source(bets.is_object(), "native free-spin line bets")?;
            ("spin".into(), Some(json!({"bets":bets})))
        } else if phase == "check_card" && allowed(reply, "finish", true) {
            ("finish".into(), None)
        } else {
            return Err(unknown_event("BGaming unrecognized v1 continuation"));
        }
    } else if family == "alice" && phase == "gamble_bonus" {
        if plan.policy.starts_with("gamble_once/") && !state.gambled {
            state.gambled = true;
            ("gamble_bonus".into(), Some(json!({})))
        } else {
            (
                "pick_cards".into(),
                Some(json!({"mode":"select_pick_cards"})),
            )
        }
    } else if family == "alice" && phase == "pick_cards" {
        state.steps += 1;
        if plan.policy.ends_with("/auto") {
            ("pick_cards".into(), Some(json!({"mode":"auto"})))
        } else {
            let candidates: Vec<_> = (0..plan.card_count)
                .filter(|i| !state.used.contains(i))
                .collect();
            let index = if plan.policy.ends_with("/manual_high") {
                candidates.last()
            } else {
                candidates.first()
            }
            .ok_or_else(|| unknown_event("BGaming no unopened Alice card"))?;
            state.used.insert(*index);
            (
                "pick_cards".into(),
                Some(json!({"mode":"any","index":index})),
            )
        }
    } else if family == "cats" && phase == "play_bonus" {
        let used = reply["features"]["bonus_data"]["prizes_data"]["all"]
            .as_array()
            .ok_or_else(|| unknown_event("BGaming Cats native opened bottle list missing"))?;
        let mut opened = BTreeSet::new();
        for item in used {
            let i = item["index"]
                .as_u64()
                .filter(|i| *i < plan.card_count as u64)
                .ok_or_else(|| unknown_event("BGaming invalid Cats bottle index"))?;
            require_source(opened.insert(i as usize), "unique Cats bottle history")?;
        }
        let index = (0..plan.card_count)
            .find(|i| !opened.contains(i) && !state.used.contains(i))
            .ok_or_else(|| unknown_event("BGaming no unopened Cats bottle"))?;
        state.used.insert(index);
        ("play_bonus".into(), Some(json!({"index":index})))
    } else if family == "wildmoon" && phase == "preselection_game" {
        if let Some(first) = plan.first_door {
            let doors = reply["features"]["preselection_game"]["doors"]
                .as_array()
                .ok_or_else(|| unknown_event("BGaming native door history missing"))?;
            let mut opened = BTreeSet::new();
            for door in doors {
                let index = door["index"]
                    .as_u64()
                    .filter(|i| *i < 16)
                    .ok_or_else(|| unknown_event("BGaming invalid opened door"))?;
                require_source(opened.insert(index as usize), "unique door history")?;
            }
            let index = if state.steps == 0 {
                first
            } else {
                (0..16)
                    .find(|i| !opened.contains(i) && !state.used.contains(i))
                    .ok_or_else(|| unknown_event("BGaming no unopened door"))?
            };
            require_source(
                !opened.contains(&index) && state.used.insert(index),
                "unopened requested door",
            )?;
            state.steps += 1;
            (
                "play_preselection_game".into(),
                Some(json!({"index":index})),
            )
        } else {
            (
                "play_preselection_game".into(),
                Some(json!({"mode":"auto"})),
            )
        }
    } else if family == "dig" && phase == "gamble" {
        if !state.gambled {
            state.gambled = true;
            ("gamble".into(), Some(json!({"bet":plan.policy})))
        } else {
            ("close".into(), None)
        }
    } else if family == "extras" && allowed(reply, "buy_extra_bonus", false) {
        let target = plan.target_extra.as_ref().unwrap();
        let price = &reply["features"]["bonus_data"]["bonus_game_prices"][target];
        if !state.extras.contains(target) && price.as_i64().is_some_and(|v| v > 0) {
            state.extras.insert(target.clone());
            ("buy_extra_bonus".into(), Some(json!({"bonus_type":target})))
        } else {
            ("freespin".into(), None)
        }
    } else {
        let next = if let Some(base) = &plan.base {
            planned_legacy_next(reply, base, source, game)?
        } else {
            legacy_next(reply)?
        };
        let Some(command) = next else {
            return Ok(None);
        };
        let options = plan
            .base
            .as_ref()
            .and_then(|b| b.continuation_options.get(command).cloned());
        (command.into(), options)
    };
    require_source(
        allowed(reply, &action.0, v1),
        "currently advertised native continuation",
    )?;
    Ok(Some(action))
}
pub(super) async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    page: Url,
    config: Value,
    html: &str,
    family: &str,
) -> Result<Completion> {
    let api = Url::parse(text(&config, "api")?)?;
    ensure!(trusted(&api), "BGaming official demo API required");
    let mut h = headers(&page);
    h.insert(
        text(&config, "csrfTokenHeaderName")?.into(),
        text(&config, "csrfTokenHeaderValue")?.into(),
    );
    let series = now_ms();
    let initial = ctx
        .json(
            "bgaming_init",
            api.as_str(),
            &json!({"command":"init","extra_data":{"round_series_id":series}}),
            h.clone(),
            false,
        )
        .await?;
    let v1 = family == "v1";
    require_source(
        if v1 {
            initial["api_version"] != "2" && initial["game"]["state"] == "idle"
        } else {
            initial["api_version"] == "2" && initial["flow"]["state"] == "ready"
        },
        "fresh native legacy family",
    )?;
    let classic = html.contains("https://cdn.bgaming-network.com/html/casino.min.js");
    let source = if v1 && classic {
        get(
            ctx,
            "bgaming_classic_source",
            "https://cdn.bgaming-network.com/html/casino.min.js",
        )
        .await?
    } else {
        legacy_source(ctx, &game.id).await?
    };
    if v1 {
        source_v1(&source, &game.id, classic)?;
    }
    let plan = plan(&initial, mode, &source, &game.id, family)?;
    require_source(
        allowed(&initial, "spin", v1),
        "advertised native paid entry",
    )?;
    ctx.save_session(&json!({"provider":"bgaming","game":game.id,"api":api.as_str(),"series":series,"source_sha256":sha256(source.as_bytes())}))?;
    let mut action = ("spin".to_owned(), Some(plan.entry.clone()));
    let mut state = Choices::default();
    let mut round = None;
    for step in 0..ctx.max_steps().min(500) {
        let mut request = json!({"command":action.0,"extra_data":{"round_series_id":series}});
        if let Some(options) = &action.1 {
            request["options"] = options.clone();
        }
        let reply = ctx
            .json(&action.0, api.as_str(), &request, h.clone(), true)
            .await?;
        if !v1 {
            require_source(
                reply["flow"]["command"] == action.0,
                "native command identity",
            )?;
            if step == 0 && family != "variants" {
                confirm_purchase(&reply, &plan.entry)?;
            }
        }
        if action.0 == "buy_extra_bonus" {
            require_source(
                reply["features"]["bonus_data"]["last_purchased_extra_bonus"]
                    == plan.target_extra.as_deref().unwrap_or(""),
                "native optional-extra confirmation",
            )?;
            state.target_observed = true;
        }
        if let Some(id) = native_id(&reply["flow"]["round_id"]) {
            require_source(
                round.as_ref().is_none_or(|v| v == &id),
                "unchanged native round",
            )?;
            round.get_or_insert(id);
        }
        let Some(next) = next(&reply, &plan, &mut state, &source, &game.id, family)? else {
            return Ok(Completion {
                native_round_id: round,
                details: json!({"family":family,"steps":step+1,"mode":mode.key}),
            });
        };
        action = next;
    }
    Err(unknown_event("BGaming legacy continuation limit"))
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
    fn v1_line_bets_are_original_native_units_and_finish_needs_no_money() {
        let initial = json!({"options":{"line_bets":[20,10],"lines":[1,2,3]}});
        assert_eq!(
            v1_entry(&initial, &mode("normal/all_default_lines"), "").unwrap(),
            json!({"bets":{"0":10,"1":10,"2":10}})
        );
        let plan = Plan {
            entry: json!({}),
            base: None,
            card_count: 0,
            policy: "normal/all_default_lines".into(),
            first_door: None,
            target_extra: None,
        };
        assert_eq!(
            next(
                &json!({"game":{"state":"check_card"},"available_commands":["finish"]}),
                &plan,
                &mut Choices::default(),
                "",
                "BookOfCats",
                "v1"
            )
            .unwrap(),
            Some(("finish".into(), None))
        );
    }
    #[test]
    fn alice_auto_and_gamble_once_are_semantic_without_balance() {
        let plan = Plan {
            entry: json!({}),
            base: None,
            card_count: 4,
            policy: "gamble_once/auto".into(),
            first_door: None,
            target_extra: None,
        };
        let mut state = Choices::default();
        let reply = json!({"flow":{"state":"gamble_bonus","available_actions":["gamble_bonus","pick_cards"]}});
        assert_eq!(
            next(&reply, &plan, &mut state, "", "AliceWonderLuck", "alice")
                .unwrap()
                .unwrap(),
            ("gamble_bonus".into(), Some(json!({})))
        );
        assert_eq!(
            next(&reply, &plan, &mut state, "", "AliceWonderLuck", "alice")
                .unwrap()
                .unwrap(),
            (
                "pick_cards".into(),
                Some(json!({"mode":"select_pick_cards"}))
            )
        );
        assert!(
            next(
                &json!({"flow":{"state":"novel","available_actions":["new"]}}),
                &plan,
                &mut state,
                "",
                "AliceWonderLuck",
                "alice"
            )
            .is_err()
        );
    }
    #[test]
    fn v1_unrelated_flag_and_zero_free_counter_cannot_dispatch_another_spin() {
        let mut p = Plan {
            entry: json!({}),
            base: None,
            card_count: 0,
            policy: "normal/all_default_lines".into(),
            first_door: None,
            target_extra: None,
        };
        let reply = json!({"game":{"state":"freespin","freespins_left":1},"available_commands":["spin"],"bets":{"lines":{"0":10}}});
        assert!(
            next(
                &reply,
                &p,
                &mut Choices::default(),
                "unrelated.isApi2;bet.bets=menu",
                "BookOfCats",
                "v1"
            )
            .is_err()
        );
        let source = r#"if(C.isApi2)F&&(q.command="freespin");else{q.bets=B}"#;
        assert!(free_spin_wire(source).unwrap());
        assert_eq!(
            next(
                &reply,
                &p,
                &mut Choices::default(),
                source,
                "BookOfCats",
                "v1"
            )
            .unwrap()
            .unwrap(),
            ("spin".into(), Some(json!({"bets":{"0":10}})))
        );
        let mut zero = reply.clone();
        zero["game"]["freespins_left"] = json!(0);
        assert!(
            next(
                &zero,
                &p,
                &mut Choices::default(),
                source,
                "BookOfCats",
                "v1"
            )
            .is_err()
        );
        p.policy = "gamble_once/manual_low".into();
        p.card_count = 4;
        let closed = json!({"flow":{"state":"closed"}});
        let e = next(
            &closed,
            &p,
            &mut Choices::default(),
            "",
            "AliceWonderLuck",
            "alice",
        )
        .unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_none());
        let mut state = Choices {
            gambled: true,
            steps: 1,
            ..Choices::default()
        };
        state.used.insert(0);
        assert!(
            next(&closed, &p, &mut state, "", "AliceWonderLuck", "alice")
                .unwrap()
                .is_none()
        );
        p.first_door = Some(3);
        assert!(
            next(
                &closed,
                &p,
                &mut Choices::default(),
                "",
                "WildMoonThieves",
                "wildmoon"
            )
            .is_err()
        );
    }
    #[test]
    fn new_v1_bonus_phase_with_finish_is_not_silently_collected() {
        let plan = Plan {
            entry: json!({}),
            base: None,
            card_count: 0,
            policy: "normal/all_default_lines".into(),
            first_door: None,
            target_extra: None,
        };
        let e = next(
            &json!({"game":{"state":"bonus_choice"},"available_commands":["finish"]}),
            &plan,
            &mut Choices::default(),
            "",
            "BookOfCats",
            "v1",
        )
        .unwrap_err();
        assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
}
