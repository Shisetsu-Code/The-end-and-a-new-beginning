use super::native::*;
use super::*;
use base64::Engine;
use std::io::Write;

fn contract(legacy: bool) -> Contract {
    Contract {
        legacy,
        omit_empty_entry: false,
        pick_bound: true,
        binding: json!({}),
    }
}
fn offer() -> Offer {
    Offer {
        id: "uuid".into(),
        selector: "extra".into(),
        bet_type: "default".into(),
        cost: 20,
        multiplier: 2,
        parameters: json!([]),
        lines: None,
    }
}

#[test]
fn modern_and_legacy_commands_keep_native_placement() {
    let o = offer();
    let modern = entry(&o, &contract(false)).unwrap();
    assert_eq!(modern["betCost"], 40);
    assert_eq!(modern["featureType"], "extra");
    assert!(modern["gameData"].get("stakeCostUuid").is_none());
    let legacy = entry(&o, &contract(true)).unwrap();
    assert_eq!(legacy["gameData"]["stakeCostUuid"], "uuid");
    assert_eq!(legacy["gameData"]["baseCostMultiplier"], 2);
    assert!(legacy.get("betCost").is_none());
}

#[test]
fn supplied_outbound_flow_commands_are_exactly_reproducible() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/playtech/command-flows.json"
    ))
    .unwrap();
    let mut count = 0;
    let mut continuations = 0;
    for flow in fixtures["flows"].as_array().unwrap() {
        for command in flow["commands"].as_array().unwrap() {
            let mut c = contract(false);
            let actual = if command["gameData"]["ID"] == 40021 {
                c.omit_empty_entry = command["gameData"].get("parameters").is_none();
                let o = Offer {
                    id: "receipt-id".into(),
                    selector: command["featureType"].as_str().unwrap().into(),
                    bet_type: command["betType"].as_str().unwrap().into(),
                    cost: command["betCost"].as_u64().unwrap(),
                    multiplier: 1,
                    parameters: command["gameData"]
                        .get("parameters")
                        .cloned()
                        .unwrap_or(json!([])),
                    lines: None,
                };
                entry(&o, &c).unwrap()
            } else {
                continuations += 1;
                continuation(&c, json!([])).unwrap()
            };
            assert_eq!(&actual, command, "{}", flow["source"]);
            count += 1;
        }
    }
    assert_eq!(count, 47);
    assert_eq!(continuations, 25);
}

#[test]
fn packed_data_is_bounded_and_rejects_trailing_or_truncated_stream() {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(br#"{"nextRound":"BET_END_STATE_IDENTIFIER"}"#)
        .unwrap();
    let compressed = z.finish().unwrap();
    let packed = |bytes: &[u8]| json!({"_type":"gpas:packed:zlib","payload":base64::engine::general_purpose::STANDARD.encode(bytes)});
    assert_eq!(decode_data(&packed(&compressed)).unwrap()["nextRound"], END);
    assert!(decode_data(&packed(&compressed[..compressed.len() - 1])).is_err());
    let mut extra = compressed.clone();
    extra.extend_from_slice(b"junk");
    assert!(decode_data(&packed(&extra)).is_err());
    assert!(decode_data(&json!({"_type":"gpas:packed:zlib","payload":"<redacted>"})).is_err());
}

#[test]
fn round_close_needs_native_terminal_and_unknown_phases_stop() {
    let c = contract(false);
    assert!(matches!(
        next_step(
            &json!({"gameRoundClosed":true,"gameData":{"nextRound":END}}),
            &c,
            &mut Picks::default(),
            &json!({})
        )
        .unwrap(),
        Step::Closed
    ));
    let e = next_step(
        &json!({"gameRoundClosed":false,"gameData":{"nextRound":"invented"}}),
        &c,
        &mut Picks::default(),
        &json!({}),
    )
    .unwrap_err();
    assert!(e.downcast_ref::<crate::model::UnknownEvent>().is_some());
    assert!(
        next_step(
            &json!({"gameRoundClosed":true,"gameData":{"nextRound":"invented"}}),
            &c,
            &mut Picks::default(),
            &json!({})
        )
        .is_err()
    );
}

#[test]
fn pick_values_are_offered_and_consumed_only_by_native_evidence() {
    let c = contract(false);
    let mut picks = Picks::default();
    let response = json!({"gameRoundClosed":false,"gameData":{"nextRound":"Choose","playStack":[{"round":"Choose","modeType":"LIST_PICK","remainingPlayCount":2}],"listPickPlayerPickOptions":[{"index":2},{"index":5}]}});
    let Step::Continue(first) = next_step(&response, &c, &mut picks, &json!({"Choose":5})).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        first["gameData"]["parameters"],
        json!([{"key":"pick","value":"5"}])
    );
    let Step::Continue(second) = next_step(&response, &c, &mut picks, &json!({})).unwrap() else {
        panic!()
    };
    assert_eq!(second["gameData"]["parameters"][0]["value"], "2");
    let mut consumed = response.clone();
    consumed["gameData"]["playStack"][0]["lastPlayInModeData"] = json!({"listPickData":{"payoutResults":[{"context":{"uiIndex":2}},{"context":{"uiIndex":5}}]}});
    assert!(next_step(&consumed, &c, &mut picks, &json!({})).is_err());
    assert!(next_step(&response, &c, &mut Picks::default(), &json!({"Choose":7})).is_err());
}

#[test]
fn config_uuid_active_option_and_multiplier_are_enforced() {
    let cfg = json!({"game":{"costConfiguration":[{"uuid":"uuid","name":"extra","cost":20}],"baseCostMultipliers":[1,2],"activeBetOptions":["extra"]}});
    let m = Mode {
        key: "uuid".into(),
        kind: ModeKind::AnteBet,
        label: String::new(),
        requires_prerequisites: Value::Null,
    };
    let c = Contract {
        binding: json!({"modes":{"uuid":{"selector":"extra","parameters":[]}}}),
        ..contract(false)
    };
    assert_eq!(
        resolve(&cfg, &m, &c, "", &json!({}), &json!({"multiplier":2}))
            .unwrap()
            .multiplier,
        2
    );
    assert!(resolve(&cfg, &m, &c, "", &json!({}), &json!({"multiplier":3})).is_err());
    let mut inactive = cfg.clone();
    inactive["game"]["activeBetOptions"] = json!([]);
    assert!(resolve(&inactive, &m, &c, "", &json!({}), &json!({})).is_err());
    let mut missing = cfg;
    missing["game"]["costConfiguration"][0]["uuid"] = json!("changed");
    assert!(resolve(&missing, &m, &c, "", &json!({}), &json!({})).is_err());
}

#[test]
fn source_hash_changes_and_special_producers_cannot_silently_dispatch() {
    assert!(
        source_contract(
            &json!({"setup":{"usesLegacyBetApi":false}}),
            &[format!("{CLASS} 40021 48775 GLU.game.playMoney")],
            &json!({"source_sha256":["wrong"]})
        )
        .is_err()
    );
    assert!(source_contract(&json!({"env":{"glu6":true}}), &[], &json!({})).is_err());
    assert!(
        source_contract(
            &json!({}),
            &[],
            &json!({"requires_complete_native_request_producer":true})
        )
        .is_err()
    );
}

#[test]
fn exact_supported_inventory_has_no_game_wide_fallback() {
    let mut count = 0;
    for (code, modes) in SUPPORTED["games"].as_object().unwrap() {
        for (id, details) in modes.as_object().unwrap() {
            let mode: Mode =
                serde_json::from_value(json!({"key":id,"type":details["kind"]})).unwrap();
            let game = Game {
                id: format!("gpas:{code}"),
                name: String::new(),
                modes: vec![mode.clone()],
            };
            assert!(supports(&game, &mode));
            count += 1;
            let mut bogus = mode;
            bogus.key = "00000000-0000-0000-0000-000000000000".into();
            assert!(!supports(&game, &bogus));
        }
    }
    assert_eq!(count, 100);
    assert_eq!(binding(CODE)["modes"][OFFER]["selector"], "default");
    assert!(SUPPORTED["games"]["gpas_extrfruits_pop"].is_null());
}

#[test]
fn source_loader_resolves_static_modern_branch_without_executing_code() {
    let js = r#"var modern='noModule' in HTMLScriptElement.prototype;var path=modern?'client.modern.js':'client.old.js';script.setAttribute('src',path);"#;
    assert_eq!(sources::dependencies(js).unwrap(), vec!["client.modern.js"]);
    assert!(
        sources::dependencies("var path=fetch('secret');s.setAttribute('src',path);")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        sources::dependencies("var s=document.createElement('script');s.src='nested.js';").unwrap(),
        vec!["nested.js"]
    );
    let root = url::Url::parse("https://gpas-games.playtechone.com/game/1/").unwrap();
    let base = root.join("packs/manifest.json").unwrap();
    assert!(sources::resource(&base, &root, "../client.js").is_ok());
    for bad in [
        "../../../escape.js",
        "https://attacker.example/code.js",
        "%2e%2e/code.js",
        "../x.js?code=1",
    ] {
        assert!(sources::resource(&base, &root, bad).is_err());
    }
}

#[test]
fn source_selectors_and_jmm_mapping_are_context_bound() {
    let costs = vec![json!({"name":"20"}), json!({"name":"30"})];
    let text =
        r#"getContext(contextNames.default,{transform:function(x){return"mode-".concat(x)}}"#;
    assert_eq!(sources::selectors(text, &costs).unwrap()["20"], "mode-20");
    assert!(
        sources::selectors(
            &text.replace("contextNames.default", "contextNames.freespins"),
            &costs
        )
        .unwrap()
        .is_empty()
    );
    let maps =
        json!({"bet_features":{"buy3":"option3"},"bet_feature_types":{"buy_feature":"^buy"}});
    assert_eq!(
        sources::mapped_kind("option3", &maps),
        Some(ModeKind::Purchase)
    );
    assert_eq!(sources::mapped_kind("not-declared", &maps), None);
}

#[test]
fn all_receipt_modes_resolve_with_fresh_scopes_and_reject_uuid_changes() {
    let receipts: Value =
        serde_json::from_str(include_str!("../../../bindings/playtech-receipts.json")).unwrap();
    let text = "VolatilityController ExtraBetController BuyFeatureModule";
    let mut count = 0;
    for (_, game) in receipts.as_object().unwrap() {
        for (id, receipt) in game["modes"].as_object().unwrap() {
            let kind = match receipt["kind"].as_str().unwrap() {
                "SPIN" => ModeKind::Normal,
                "PURCHASE" => ModeKind::Purchase,
                "ANTE_BET" => ModeKind::AnteBet,
                _ => panic!("unknown receipt kind"),
            };
            let name = receipt["name"].as_str().unwrap();
            let mode = Mode {
                key: id.clone(),
                kind,
                label: String::new(),
                requires_prerequisites: Value::Null,
            };
            let cfg = json!({"game":{"costConfiguration":[{"uuid":id,"name":name,"cost":10}],"baseCostMultipliers":[1,2],"namedScopes":{name:{"betType":"default","featureType":name}},"featureBetEnabled":true}});
            let o = resolve(&cfg, &mode, &contract(false), text, &json!({}), &json!({})).unwrap();
            assert_eq!(entry(&o, &contract(false)).unwrap()["featureType"], name);
            count += 1;
        }
    }
    assert_eq!(count, 32);
}

#[test]
fn free_round_and_fixed_pick_preserve_wire_shape_without_wallet_fields() {
    let mut c = contract(false);
    let response = json!({"gameRoundClosed":false,"gameData":{"nextRound":"FreeGame","playStack":[{"round":"FreeGame","remainingPlayCount":3,"modeType":"SLOTS"}]}});
    let Step::Continue(command) =
        next_step(&response, &c, &mut Picks::default(), &Value::Null).unwrap()
    else {
        panic!()
    };
    assert_eq!(command, continuation(&c, json!([])).unwrap());
    c.binding = json!({"fixed_continuation_parameters":{"Wheel":[{"key":"pick","value":"WheelPickRequest"}]}});
    let mut wheel = response;
    wheel["gameData"]["nextRound"] = json!("Wheel");
    wheel["gameData"]["playStack"][0] =
        json!({"round":"Wheel","remainingPlayCount":1,"modeType":"LIST_PICK"});
    let Step::Continue(command) =
        next_step(&wheel, &c, &mut Picks::default(), &Value::Null).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        command["gameData"]["parameters"][0]["value"],
        "WheelPickRequest"
    );
    wheel["gameData"]["listPickPlayerPickOptions"] = json!([{"index":1}]);
    assert!(next_step(&wheel, &c, &mut Picks::default(), &Value::Null).is_err());
}

#[test]
fn numeric_pick_and_parameter_suffix_and_legacy_terminal_are_checked() {
    let mut c = contract(true);
    c.binding = json!({"numeric_pick_parameters":true,"continuation_parameter_suffix":[{"key":"baseCostMultiplier","value":1}],"terminal_rounds":["Base"]});
    let cmd = continuation(&c, json!([{"key":"pick","value":"2"}])).unwrap();
    assert_eq!(cmd["gameData"]["parameters"][0]["value"], 2);
    assert!(cmd.get("autoplay").is_none());
    assert!(continuation(&c, json!([{"key":"baseCostMultiplier","value":1}])).is_err());
    let good = json!({"gameRoundClosed":true,"gameData":{"nextRound":"Base","playStack":[{"remainingPlayCount":0,"isNextPlayMode":false,"isLastPlayMode":true}]}});
    assert!(matches!(
        next_step(&good, &c, &mut Picks::default(), &Value::Null).unwrap(),
        Step::Closed
    ));
    let mut bad = good;
    bad["gameData"]["playStack"][0]["remainingPlayCount"] = json!(1);
    assert!(next_step(&bad, &c, &mut Picks::default(), &Value::Null).is_err());
}

#[test]
fn observed_baseline_rejects_old_uuid_and_changed_native_profile() {
    let c = Contract {
        binding: binding("gpas_wofire1_pop"),
        ..contract(false)
    };
    let m: Mode = serde_json::from_value(
        json!({"key":"102ec212-5088-4bb0-abf3-cefe3de1bf80","type":"normal"}),
    )
    .unwrap();
    let cfg = json!({"game":{"extraBetEnabled":false,"costConfiguration":[{"uuid":m.key,"name":"Extra","cost":20,"parameters":{"winline":"30"}}],"baseCostMultipliers":[1,2]},"limits":{"minBet":20}});
    let o = resolve(&cfg, &m, &c, "", &json!({}), &json!({})).unwrap();
    assert_eq!(o.selector, "default");
    assert_eq!(o.parameters, json!([]));
    assert!(resolve(&cfg, &m, &c, "", &json!({}), &json!({"multiplier":2})).is_err());
    let mut changed = cfg;
    changed["game"]["extraBetEnabled"] = json!(true);
    assert!(resolve(&changed, &m, &c, "", &json!({}), &json!({})).is_err());
}

#[test]
fn source_pin_accepts_exact_content_and_disabled_feature_stops_entry() {
    let source = format!("{CLASS} 40021 48775 GLU.game.playMoney FGPickController 'pick'");
    let pin = crate::model::sha256(source.as_bytes());
    let c = source_contract(
        &json!({"setup":{"usesLegacyBetApi":false}}),
        &[source],
        &json!({"source_sha256":[pin],"modes":{"uuid":{"selector":"BuyFeature","parameters":[]}}}),
    )
    .unwrap();
    let mode: Mode = serde_json::from_value(json!({"key":"uuid","type":"purchase"})).unwrap();
    let cfg = json!({"game":{"featureBetEnabled":false,"costConfiguration":[{"uuid":"uuid","name":"BuyFeature","cost":100}],"baseCostMultipliers":[1]}});
    assert!(resolve(&cfg, &mode, &c, "", &json!({}), &json!({})).is_err());
}

#[test]
fn truncated_stacks_unsupported_modes_and_unknown_pick_domains_stay_partial() {
    let c = contract(false);
    for data in [
        json!({"nextRound":"Free","playStack":["...x1"]}),
        json!({"nextRound":"Free","playStack":[{"round":"Free","remainingPlayCount":1,"modeType":"FUTURE_MODE"}]}),
        json!({"nextRound":"Pick","playStack":[{"round":"Pick","remainingPlayCount":1,"modeType":"LIST_PICK"}]}),
    ] {
        let error = next_step(
            &json!({"gameRoundClosed":false,"gameData":data}),
            &c,
            &mut Picks::default(),
            &Value::Null,
        )
        .unwrap_err();
        assert!(error.downcast_ref::<crate::model::UnknownEvent>().is_some());
    }
}

#[test]
fn scoped_configuration_merges_in_order_and_replaces_arrays() {
    let rows = vec![
        json!({"scope":{},"content":{"game":{"baseCostMultipliers":[1,2,3],"enabled":true,"costConfiguration":[{"uuid":"base"}]}}}),
        json!({"scope":{"betType":"default"},"content":{"game":{"currency":"EUR"}}}),
        json!({"scope":{"betType":["default","freespin"],"featureType":"buy"},"content":{"game":{"baseCostMultipliers":[2],"costConfiguration":[{"uuid":"purchase"}]}}}),
        json!({"scope":{"betType":"freespin"},"content":{"game":{"enabled":false}}}),
    ];
    let base = configuration(&rows, &json!({"betType":"default"})).unwrap();
    assert_eq!(base["game"]["baseCostMultipliers"], json!([1, 2, 3]));
    let scoped = configuration(&rows, &json!({"betType":"default","featureType":"buy"})).unwrap();
    assert_eq!(scoped["game"]["baseCostMultipliers"], json!([2]));
    assert_eq!(
        scoped["game"]["costConfiguration"],
        json!([{"uuid":"purchase"}])
    );
    assert_eq!(scoped["game"]["enabled"], true);
    assert_eq!(scoped["game"]["currency"], "EUR");
}

#[test]
fn review_minimum_native_context_only_rejects_larger_pigeon_multiplier() {
    let c = Contract {
        binding: binding("gpas_pigeonfspp_pop"),
        ..contract(false)
    };
    let id = "b450dee7-7571-47fc-af3a-4ba68e618fe8";
    assert_eq!(c.binding["minimum_native_context_only"], true);
    let mode: Mode = serde_json::from_value(json!({"key":id,"type":"purchase"})).unwrap();
    let cfg = json!({"game":{"costConfiguration":[{"uuid":id,"name":"BuyBonus","cost":650}],"baseCostMultipliers":[1,2]}});
    let o = resolve(&cfg, &mode, &c, "", &json!({}), &json!({"multiplier":1})).unwrap();
    assert_eq!(
        entry(&o, &c).unwrap()["gameData"]["parameters"],
        c.binding["modes"][id]["parameters"]
    );
    assert!(resolve(&cfg, &mode, &c, "", &json!({}), &json!({"multiplier":2})).is_err());
}

#[test]
fn review_native_scoped_bounds_select_legal_minimum_and_reject_maximum() {
    let c = Contract {
        binding: json!({"modes":{"uuid":{"selector":"buy","parameters":[]}}}),
        ..contract(false)
    };
    let mode: Mode = serde_json::from_value(json!({"key":"uuid","type":"purchase"})).unwrap();
    let cfg = json!({"game":{"costConfiguration":[{"uuid":"uuid","name":"buy","cost":650}],"baseCostMultipliers":[1,2,3]},"limits":{"minBet":1300,"maxBet":1300}});
    let o = resolve(&cfg, &mode, &c, "", &json!({}), &json!({})).unwrap();
    assert_eq!(o.multiplier, 2);
    assert_eq!(entry(&o, &c).unwrap()["betCost"], 1300);
    assert!(resolve(&cfg, &mode, &c, "", &json!({}), &json!({"multiplier":3})).is_err());
}

#[test]
fn review_reoffered_gamble_choice_is_not_consumed_without_native_evidence() {
    let c = Contract {
        binding: binding("gpas_mwilds_pop"),
        ..contract(false)
    };
    assert_eq!(c.binding["pick_domains"]["GamblePick"], json!([1, 2]));
    let mut picks = Picks::default();
    let mut reply = json!({"gameRoundClosed":false,"gameData":{"nextRound":"GamblePick","playStack":[{"round":"GamblePick","modeType":"LIST_PICK","remainingPlayCount":1}],"listPickPlayerPickOptions":[{"index":1}]}});
    for _ in 0..2 {
        let Step::Continue(cmd) = next_step(&reply, &c, &mut picks, &Value::Null).unwrap() else {
            panic!()
        };
        assert_eq!(cmd["gameData"]["parameters"][0]["value"], "1");
    }
    reply["gameData"]["playStack"][0]["lastPlayInModeData"] =
        json!({"listPickData":{"payoutResults":[{"context":{"uiIndex":1}}]}});
    assert!(next_step(&reply, &c, &mut picks, &Value::Null).is_err());
}

#[test]
fn review_venetian_masks_ferox_family_is_not_advertised_as_ogw() {
    let b = binding("gpas_venemasks_pop");
    assert!(b.to_string().contains("ferox.game.playMoney"));
    let mode: Mode = serde_json::from_value(
        json!({"key":"a9568c65-9fe0-446f-9842-311fd360d93d","type":"normal"}),
    )
    .unwrap();
    let game = Game {
        id: "gpas:gpas_venemasks_pop".into(),
        name: String::new(),
        modes: vec![mode.clone()],
    };
    assert!(!supports(&game, &mode));
}

#[test]
fn review_effective_feature_scope_owns_legal_stakes_not_base_scope() {
    let c = Contract {
        binding: json!({"modes":{"uuid":{"selector":"buy","parameters":[]}}}),
        ..contract(false)
    };
    let mode: Mode = serde_json::from_value(json!({"key":"uuid","type":"purchase"})).unwrap();
    let rows = vec![
        json!({"scope":{},"content":{"game":{"costConfiguration":[{"uuid":"uuid","name":"buy","cost":650}],"baseCostMultipliers":[1,2,3]},"limits":{"minBet":650,"maxBet":650}}}),
        json!({"scope":{"betType":"default","featureType":"buy"},"content":{"limits":{"minBet":1300,"maxBet":1950}}}),
    ];
    let o = resolve_scoped(&rows, &mode, &c, "", &json!({}), &json!({})).unwrap();
    assert_eq!(o.multiplier, 2);
    assert_eq!(
        resolve_scoped(&rows, &mode, &c, "", &json!({}), &json!({"multiplier":3}))
            .unwrap()
            .multiplier,
        3
    );
    assert!(resolve_scoped(&rows, &mode, &c, "", &json!({}), &json!({"multiplier":1})).is_err());
    let fallback = json!({"game":{"costConfiguration":[{"uuid":"uuid","name":"buy","cost":650}],"baseCostMultipliers":[1,2,3],"minBet":1300,"maxBet":1300}});
    assert_eq!(
        resolve(&fallback, &mode, &c, "", &json!({}), &Value::Null)
            .unwrap()
            .multiplier,
        2
    );
}
