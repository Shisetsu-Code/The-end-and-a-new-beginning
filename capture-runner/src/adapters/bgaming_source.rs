//! Static client metadata extraction, ported from native_variants.py. Never evaluates JS.
use super::*;

pub(super) fn balanced(text: &str, start: usize) -> Result<&str> {
    let bytes = text.as_bytes();
    let opening = *bytes
        .get(start)
        .ok_or_else(|| unknown_event("BGaming source range missing"))?;
    let closing = match opening {
        b'{' => b'}',
        b'[' => b']',
        _ => return Err(unknown_event("BGaming literal container required")),
    };
    let mut depth = 0usize;
    let mut quote = None;
    let mut escape = false;
    for (i, &c) in bytes.iter().enumerate().skip(start) {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if c == b'\\' {
                escape = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if matches!(c, b'"' | b'\'' | b'`') {
            quote = Some(c);
            continue;
        }
        if c == opening {
            depth += 1;
        } else if c == closing {
            depth = depth
                .checked_sub(1)
                .ok_or_else(|| unknown_event("BGaming unbalanced source"))?;
            if depth == 0 {
                return Ok(&text[start..=i]);
            }
        }
    }
    Err(unknown_event("BGaming unclosed source literal"))
}
pub(super) fn fields(object: &str) -> Result<BTreeMap<String, String>> {
    require_source(
        object.starts_with('{') && object.ends_with('}'),
        "static object",
    )?;
    let mut result = BTreeMap::new();
    let mut depth = 0i32;
    let mut quote = None;
    let mut escape = false;
    let mut start = 1;
    for (i, c) in object.bytes().enumerate().skip(1).take(object.len() - 2) {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if c == b'\\' {
                escape = true;
            } else if c == q {
                quote = None;
            }
        } else if matches!(c, b'"' | b'\'' | b'`') {
            quote = Some(c);
        } else if matches!(c, b'[' | b'{' | b'(') {
            depth += 1;
        } else if matches!(c, b']' | b'}' | b')') {
            depth -= 1;
        } else if c == b',' && depth == 0 {
            insert_field(&mut result, &object[start..i])?;
            start = i + 1;
        }
    }
    insert_field(&mut result, &object[start..object.len() - 1])?;
    Ok(result)
}
fn insert_field(out: &mut BTreeMap<String, String>, part: &str) -> Result<()> {
    if part.trim().is_empty() {
        return Ok(());
    }
    let mut quote = None;
    let mut escape = false;
    let mut colon = None;
    for (i, c) in part.bytes().enumerate() {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if c == b'\\' {
                escape = true;
            } else if c == q {
                quote = None;
            }
        } else if matches!(c, b'"' | b'\'') {
            quote = Some(c);
        } else if c == b':' {
            colon = Some(i);
            break;
        }
    }
    let at = colon.ok_or_else(|| unknown_event("BGaming computed source field"))?;
    let key = part[..at].trim().trim_matches('"');
    let value = part[at + 1..].trim();
    require_source(
        !key.is_empty() && !out.contains_key(key),
        "unique literal property",
    )?;
    out.insert(key.into(), value.trim().into());
    Ok(())
}
pub(super) fn literal(source: &str, value: &str) -> Result<Value> {
    let value = value.trim();
    if let Ok(v) = serde_json::from_str::<Value>(value) {
        return Ok(v);
    }
    match value {
        "!0" => return Ok(json!(true)),
        "!1" => return Ok(json!(false)),
        _ => {}
    }
    if let Some(v) = value
        .strip_prefix("0x")
        .and_then(|v| u64::from_str_radix(v, 16).ok())
    {
        return Ok(json!(v));
    }
    require_source(
        Regex::new(r"^[A-Za-z_$][\w$]*$")?.is_match(value),
        "literal scalar or variable",
    )?;
    let pattern = Regex::new(&format!(r"\b{}\s*=([^,;]+)", regex::escape(value)))?;
    let mut values = BTreeSet::new();
    for capture in pattern.captures_iter(source) {
        let prefix = source[..capture.get(0).unwrap().start()].trim_end();
        if prefix.ends_with('.') {
            continue;
        }
        let raw = capture[1].trim();
        let parsed: Value = serde_json::from_str(raw)
            .map_err(|_| unknown_event("BGaming nonliteral or mutable source constant"))?;
        require_source(
            parsed.is_string() || parsed.is_number(),
            "scalar source constant",
        )?;
        values.insert(raw.to_owned());
    }
    require_source(values.len() == 1, "unique source constant")?;
    serde_json::from_str(values.first().unwrap()).map_err(|e| unknown_event(e.to_string()))
}
pub(super) fn variant_entry(initial: &Value, mode: &Mode, source: &str) -> Result<Value> {
    let assignment =
        Regex::new(r"\b[A-Za-z_$][\w$]*\.mode=([A-Za-z_$][\w$]*)\.currentVariantId\.toString\(\)")?
            .captures(source)
            .ok_or_else(|| unknown_event("BGaming variant selector absent"))?;
    require_source(Regex::new(&format!(r"\b[A-Za-z_$][\w$]*\.bet=[A-Za-z_$][\w$]*\.data\.bet/{}\.currentConfig\.base_bet\*[A-Za-z_$][\w$]*\[0\]\.base_bet",regex::escape(&assignment[1])))?.is_match(source),"native variant base-bet conversion")?;
    let array_names: BTreeSet<String> = Regex::new(r#""allOptions",([A-Za-z_$][\w$]*)\)"#)?
        .captures_iter(source)
        .map(|c| c[1].to_owned())
        .collect();
    require_source(array_names.len() == 1, "unique variant array")?;
    let array_name = array_names.first().unwrap();
    let pattern = Regex::new(&format!(
        r"(?:^|[^\w$.]){}=\[([A-Za-z0-9_$,]+)\]",
        regex::escape(array_name)
    ))?;
    let arrays: Vec<_> = pattern
        .captures_iter(source)
        .filter(|c| {
            let start =
                c.get(0).unwrap().start() + c.get(0).unwrap().as_str().find(array_name).unwrap();
            !source[..start].trim_end().ends_with('.')
        })
        .collect();
    require_source(
        arrays.len() == 1,
        "unique non-property variant array assignment",
    )?;
    let array = &arrays[0];
    for (index, name) in array[1].split(',').enumerate() {
        let cfg = variant_config(source, name, &mut BTreeSet::new())?;
        let title = text(&cfg, "name")?;
        let args = cfg
            .get("request_options")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let mut request = args.clone();
        request["bet"] = json!(minimum(&initial["options"]["available_bets"])?);
        if request.get("mode").is_none() {
            request["mode"] = json!(index.to_string());
        }
        if mode.key == title {
            active_feature(initial, &request)?;
            return Ok(request);
        }
        if args.get("purchased_feature").is_some() {
            continue;
        }
        for buy in cfg["buys"].as_array().into_iter().flatten() {
            let feature = text(buy, "name")?;
            if mode.key == format!("{title}/{feature}") {
                request["purchased_feature"] = json!(feature);
                active_feature(initial, &request)?;
                return Ok(request);
            }
        }
    }
    Err(unknown_event(
        "BGaming variant no longer defined by current client",
    ))
}
fn active_feature(initial: &Value, request: &Value) -> Result<()> {
    if let Some(feature) = request.get("purchased_feature") {
        require_source(
            !initial["options"]["feature_options"]["disabled_features"]
                .as_array()
                .is_some_and(|a| a.contains(feature)),
            "active native variant feature",
        )?;
    }
    Ok(())
}
fn variant_config(source: &str, name: &str, trail: &mut BTreeSet<String>) -> Result<Value> {
    require_source(
        trail.len() < 20 && trail.insert(name.into()),
        "nonrecursive variant inheritance",
    )?;
    let pattern = Regex::new(&format!(
        r"(?:^|[^\w$.]){}=(?:[A-Za-z_$][\w$]*\(([A-Za-z_$][\w$]*),)?\{{",
        regex::escape(name)
    ))?;
    let matches: Vec<_> = pattern
        .captures_iter(source)
        .filter(|c| {
            let start = c.get(0).unwrap().start() + c.get(0).unwrap().as_str().find(name).unwrap();
            !source[..start].trim_end().ends_with('.')
        })
        .collect();
    require_source(
        matches.len() == 1,
        "unique non-property native variant object",
    )?;
    let found = &matches[0];
    let mut result = if let Some(parent) = found.get(1) {
        variant_config(source, parent.as_str(), trail)?
    } else {
        json!({})
    };
    let start = found.get(0).unwrap().end() - 1;
    let f = fields(balanced(source, start)?)?;
    for key in ["name", "base_bet", "bet_adding"] {
        if let Some(v) = f.get(key) {
            result[key] = serde_json::from_str(v).map_err(|e| {
                unknown_event(format!("BGaming variant field must be a JSON literal: {e}"))
            })?;
        }
    }
    require_source(
        result["base_bet"].as_f64().is_some_and(|v| v > 0.0) && result["name"].is_string(),
        "complete native variant",
    )?;
    if let Some(v) = f.get("requestOptions") {
        let mut args = json!({});
        for (key, value) in fields(v)? {
            require_source(
                matches!(
                    key.as_str(),
                    "purchased_feature" | "purchased_feature_level" | "mode"
                ),
                "native variant request field",
            )?;
            args[&key] = serde_json::from_str(&value).map_err(|e| {
                unknown_event(format!(
                    "BGaming variant request field must be literal: {e}"
                ))
            })?;
        }
        result["request_options"] = args;
    }
    if let Some(v) = f.get("buy_features") {
        let re = Regex::new(r#"name:"([A-Za-z0-9_]+)",price:(\d+(?:\.\d+)?)"#)?;
        let buys: Vec<_> = re.captures_iter(v).map(|c| json!({"name":&c[1]})).collect();
        require_source(
            !v.contains("features:[") || !buys.is_empty(),
            "explicit variant products",
        )?;
        result["buys"] = json!(buys);
    }
    trail.remove(name);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_fields_do_not_execute_nested_code_or_split_quoted_delimiters() {
        let value = r#"{name:"test",custom:{nested:"} unsafe()"},base_bet:20}"#;
        assert_eq!(balanced(value, 0).unwrap(), value);
        assert_eq!(fields(value).unwrap()["base_bet"], "20");
        assert!(literal("", "process.exit()").is_err());
    }
    #[test]
    fn source_variant_retains_native_base_bet_and_exact_mode_index() {
        let source = r#"var A={name:"20 Lines",base_bet:20,buy_features:{features:[{name:"bonus_buy",price:60}]}},B=extend(A,{name:"40 Lines",base_bet:40}),ALL=[A,B];m.mode=v.currentVariantId.toString();o.bet=x.data.bet/v.currentConfig.base_bet*ALL[0].base_bet;register("allOptions",ALL)"#;
        let initial = json!({"options":{"available_bets":[40,20],"feature_options":{"disabled_features":[]}}});
        let mode = Mode {
            key: "40 Lines/bonus_buy".into(),
            kind: ModeKind::Purchase,
            label: String::new(),
            requires_prerequisites: Value::Null,
        };
        assert_eq!(
            variant_entry(&initial, &mode, source).unwrap(),
            json!({"bet":20,"mode":"1","purchased_feature":"bonus_buy"})
        );
    }
    #[test]
    fn property_assignment_cannot_become_a_native_variant_constant() {
        assert!(literal(r#"var MODE=0x0;obj.MODE="1";"#, "MODE").is_err());
        assert_eq!(
            literal(r#"var MODE="native";obj.MODE="other";"#, "MODE").unwrap(),
            json!("native")
        );
        assert!(literal(r#"var MODE="native";MODE=unknown();"#, "MODE").is_err());
    }
    #[test]
    fn variant_arrays_and_configs_reject_properties_and_ambiguous_definitions() {
        let base = r#"var A={name:"base",base_bet:20},ALL=[A];m.mode=v.currentVariantId.toString();o.bet=x.data.bet/v.currentConfig.base_bet*ALL[0].base_bet;register("allOptions",ALL)"#;
        let initial = json!({"options":{"available_bets":[20]}});
        let mode = Mode {
            key: "base".into(),
            kind: ModeKind::Normal,
            label: String::new(),
            requires_prerequisites: Value::Null,
        };
        assert!(variant_entry(&initial, &mode, base).is_ok());
        assert!(variant_entry(&initial, &mode, &base.replace("ALL=[A]", "obj.ALL=[A]")).is_err());
        assert!(variant_entry(&initial, &mode, &format!("{base};ALL=[A]")).is_err());
        assert!(variant_entry(&initial, &mode, &base.replace("var A=", "var obj.A=")).is_err());
        assert!(
            variant_entry(
                &initial,
                &mode,
                &format!(r#"{base};A={{name:"other",base_bet:20}}"#)
            )
            .is_err()
        );
    }
}
