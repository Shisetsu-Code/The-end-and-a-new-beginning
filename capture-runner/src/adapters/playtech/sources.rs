//! Bounded static source/config inspection. Downloaded JavaScript is never executed.
use super::*;
use regex::Regex;
use std::collections::{BTreeSet, VecDeque};
use url::Url;
fn re(s: &str) -> Regex {
    Regex::new(s).expect("static GPAS regex")
}

pub(super) fn resource(base: &Url, root: &Url, path: &str) -> Result<Url> {
    // Reject encoded separators/traversal rather than giving them alternate semantics.
    ensure!(
        !path.contains(['\\', '%', '?', '#']),
        "GPAS unsafe asset path"
    );
    let url = base.join(path)?;
    ensure!(
        url.origin() == root.origin()
            && url.path().starts_with(root.path())
            && url.username().is_empty()
            && url.password().is_none(),
        "GPAS source outside game root"
    );
    Ok(url)
}

#[derive(Clone, Debug)]
enum Literal {
    Text(String),
    Bool(bool),
}
#[derive(Clone, Debug)]
enum Token {
    Value(Literal),
    Name(String),
    Symbol(char),
}
fn tokenize(expression: &str) -> Result<Vec<Token>> {
    let mut chars = expression.chars().peekable();
    let mut tokens = vec![];
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        if ch == '\'' || ch == '"' {
            let mut s = String::new();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == ch {
                    closed = true;
                    break;
                }
                if c == '\\' {
                    let escaped = chars.next().context("GPAS truncated source string")?;
                    ensure!(
                        matches!(escaped, '\\' | '\'' | '"' | '/'),
                        "GPAS dynamic source escape"
                    );
                    s.push(escaped)
                } else {
                    s.push(c)
                }
            }
            ensure!(closed, "GPAS truncated source string");
            tokens.push(Token::Value(Literal::Text(s)));
        } else if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' {
            let mut name = ch.to_string();
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
            {
                name.push(chars.next().unwrap())
            }
            tokens.push(Token::Name(name));
        } else {
            ensure!("+?:()".contains(ch), "GPAS unsupported loader expression");
            tokens.push(Token::Symbol(ch))
        }
        ensure!(tokens.len() <= 512, "GPAS loader expression too large");
    }
    Ok(tokens)
}
fn expression(text: &str, env: &BTreeMap<String, Literal>) -> Result<Literal> {
    fn atom(t: &[Token], i: &mut usize, env: &BTreeMap<String, Literal>) -> Result<Literal> {
        let token = t.get(*i).context("GPAS truncated loader")?;
        *i += 1;
        match token {
            Token::Value(v) => Ok(v.clone()),
            Token::Name(n) => env.get(n).cloned().context("GPAS unknown loader variable"),
            Token::Symbol('(') => {
                let v = ternary(t, i, env)?;
                ensure!(
                    matches!(t.get(*i), Some(Token::Symbol(')'))),
                    "GPAS loader parentheses"
                );
                *i += 1;
                Ok(v)
            }
            _ => anyhow::bail!("GPAS dynamic loader atom"),
        }
    }
    fn concat(t: &[Token], i: &mut usize, env: &BTreeMap<String, Literal>) -> Result<Literal> {
        let mut v = atom(t, i, env)?;
        while matches!(t.get(*i), Some(Token::Symbol('+'))) {
            *i += 1;
            let next = atom(t, i, env)?;
            let (Literal::Text(left), Literal::Text(right)) = (&mut v, next) else {
                anyhow::bail!("GPAS non-string loader concatenation")
            };
            left.push_str(&right);
            ensure!(left.len() <= 2048, "GPAS loader path too long");
        }
        Ok(v)
    }
    fn ternary(t: &[Token], i: &mut usize, env: &BTreeMap<String, Literal>) -> Result<Literal> {
        let v = concat(t, i, env)?;
        if matches!(t.get(*i), Some(Token::Symbol('?'))) {
            *i += 1;
            let yes = concat(t, i, env)?;
            ensure!(
                matches!(t.get(*i), Some(Token::Symbol(':'))),
                "GPAS loader ternary"
            );
            *i += 1;
            let no = concat(t, i, env)?;
            let Literal::Bool(condition) = v else {
                anyhow::bail!("GPAS non-boolean loader condition")
            };
            Ok(if condition { yes } else { no })
        } else {
            Ok(v)
        }
    }
    let t = tokenize(text)?;
    let mut i = 0;
    let value = ternary(&t, &mut i, env)?;
    ensure!(i == t.len(), "GPAS loader trailing code");
    Ok(value)
}

pub(super) fn dependencies(source: &str) -> Result<Vec<String>> {
    let mut paths = BTreeSet::new();
    for c in re(r#"setAttribute\(["']src["'],\s*["']([^"']+\.js)["']\)"#).captures_iter(source) {
        paths.insert(c[1].to_string());
    }
    for created in re(r#"\b([A-Za-z_$][\w$]*)\s*=\s*(?:\([^;\n{}]{0,512},\s*)?document\.createElement\(["']script["']\)"#).captures_iter(source) {
        let assignment=re(&format!(r#"(?:^|[^\w$.]){}\.src\s*=\s*["']([^"'\n]{{1,2048}}\.js)["']\s*[,;}})]"#,regex::escape(&created[1])));
        for c in assignment.captures_iter(source){paths.insert(c[1].to_string());}
    }
    if source.len() <= 32768 {
        let mut env = BTreeMap::from([
            ("true".into(), Literal::Bool(true)),
            ("false".into(), Literal::Bool(false)),
        ]);
        for c in
            re(r#"\b([A-Za-z_$][\w$]*)\s*=\s*["']noModule["']\s*in\s+HTMLScriptElement\.prototype"#)
                .captures_iter(source)
        {
            env.insert(c[1].into(), Literal::Bool(true));
        }
        for c in re(r"\b([A-Za-z_$][\w$]*)\s*=([^,;\n]{1,1024})")
            .captures_iter(source)
            .take(256)
        {
            if let Ok(v) = expression(c[2].trim(), &env) {
                env.insert(c[1].into(), v);
            }
        }
        for c in re(r#"setAttribute\(["']src["'],\s*([A-Za-z_$][\w$]*)\)"#).captures_iter(source) {
            if let Some(Literal::Text(path)) = env.get(&c[1])
                && path.ends_with(".js")
            {
                paths.insert(path.clone());
            }
        }
    }
    ensure!(paths.len() <= 16, "GPAS source graph limit");
    Ok(paths.into_iter().collect())
}

pub(super) async fn load(
    ctx: &RunContext,
    root: &Url,
    manifest: &Value,
) -> Result<(Vec<String>, Value)> {
    ensure!(
        manifest["env"]["glu6"] != true,
        "GPAS GLU6 source bridge unsupported"
    );
    let base = root.join("packs/manifest.json")?;
    let declared = manifest["javascript"]
        .as_array()
        .context("GPAS source manifest missing")?;
    let mut pending = VecDeque::new();
    for v in declared {
        pending.push_back(resource(
            &base,
            root,
            v.as_str().context("GPAS invalid script path")?,
        )?)
    }
    let mut seen = BTreeSet::new();
    let mut sources = vec![];
    let mut total = 0;
    while let Some(url) = pending.pop_front() {
        if !seen.insert(url.to_string()) {
            continue;
        }
        ensure!(seen.len() <= 16, "GPAS source graph limit");
        let source = get(ctx, "client_source", url.as_str()).await?;
        total += source.len();
        ensure!(
            source.len() <= 16 * 1024 * 1024 && total <= 64 * 1024 * 1024,
            "GPAS source size limit"
        );
        for path in dependencies(&source)? {
            pending.push_back(resource(&base, root, &path)?)
        }
        sources.push(source);
    }
    ensure!(!sources.is_empty(), "GPAS client sources missing");
    let mut projects = BTreeSet::new();
    for s in &sources {
        for c in
            re(r#"super\([^;()]{1,128},["'](jmm/[A-Za-z0-9_./%-]+\.jmp)["']\)"#).captures_iter(s)
        {
            projects.insert(c[1].to_string());
        }
        for c in re(r#"this\.[A-Za-z_$][\w$]*=["'](generic/[A-Za-z0-9_./%-]+\.jmp)["']"#)
            .captures_iter(s)
        {
            projects.insert(format!("resources/{}", &c[1]));
        }
    }
    ensure!(projects.len() <= 1, "GPAS primary project ambiguous");
    let mut maps = json!({});
    if let Some(project) = projects.first() {
        let url = resource(&base, root, project)?;
        let p: Value = serde_json::from_str(&get(ctx, "client_project", url.as_str()).await?)?;
        let module = p["modules"]
            .get("unknown")
            .or_else(|| p["modules"].get("desktop"))
            .context("GPAS desktop project missing")?;
        let paths = module["jmms"]
            .as_array()
            .context("GPAS project config missing")?;
        ensure!(paths.len() <= 128, "GPAS project config limit");
        let mut seen = BTreeSet::new();
        for path in paths {
            let path = path.as_str().context("GPAS config path invalid")?;
            ensure!(
                re(r"^[A-Za-z0-9_./%-]+\.jmm$").is_match(path),
                "GPAS config extension invalid"
            );
            if !seen.insert(path) {
                continue;
            }
            let path = if project.starts_with("resources/") {
                format!("jmm/{path}")
            } else {
                path.into()
            };
            let target = resource(&url, root, &path)?;
            let raw = get(ctx, "client_adapter_config", target.as_str()).await?;
            ensure!(raw.len() <= 8 * 1024 * 1024, "GPAS adapter config limit");
            let doc: Value = serde_json::from_str(&raw)?;
            merge_maps(&mut maps, &doc)?;
        }
    }
    Ok((sources, maps))
}
fn merge_maps(result: &mut Value, document: &Value) -> Result<()> {
    let Some(adapter) = document["config"].get("game_adapter") else {
        return Ok(());
    };
    ensure!(adapter.is_object(), "GPAS adapter map invalid");
    for key in ["bet_features", "bet_feature_types"] {
        if let Some(value) = adapter.get(key) {
            let object = value.as_object().context("GPAS adapter map not object")?;
            ensure!(
                object.len() <= 256
                    && object
                        .iter()
                        .all(|(k, v)| k.len() <= 128 && v.as_str().is_some_and(|s| s.len() <= 256)),
                "GPAS adapter map invalid"
            );
            if result.get(key).is_none() {
                result[key] = json!({})
            }
            result[key].as_object_mut().unwrap().extend(object.clone());
        }
    }
    Ok(())
}
pub(super) fn mapped_kind(name: &str, maps: &Value) -> Option<ModeKind> {
    let aliases: Vec<_> = maps["bet_features"]
        .as_object()?
        .iter()
        .filter(|(_, v)| *v == name)
        .map(|(k, _)| k)
        .collect();
    if aliases.len() != 1 {
        return None;
    }
    let mut kinds = BTreeSet::new();
    for (kind, pattern) in maps["bet_feature_types"].as_object()? {
        let p = pattern.as_str()?;
        if re(r"^[A-Za-z0-9_\-^$]+(?:\[[A-Za-z0-9_|]+\])?$").is_match(p)
            && Regex::new(p).ok()?.is_match(aliases[0])
        {
            kinds.insert(kind.as_str());
        }
    }
    if kinds.len() != 1 {
        return None;
    }
    match *kinds.first()? {
        "regular" => Some(ModeKind::Normal),
        "buy_feature" | "lucky_box" => Some(ModeKind::Purchase),
        "extra_bet" => Some(ModeKind::AnteBet),
        _ => None,
    }
}
pub(super) fn declared_kind(name: &str, text: &str) -> Option<ModeKind> {
    let mut found = vec![];
    for c in re(r#"baseCostName:["']([^"']+)["'],(?:gpasFeatureType:["'][^"']+["'],)?betParams:\{betType:[\w$.]+\.(DEFAULT|BONUS|EXTRA)([^}]{0,160})\}"#).captures_iter(text){if &c[1]==name{let k=if re(r"extraBetNum\s*:\s*[1-9]").is_match(&c[3]){ModeKind::AnteBet}else{match &c[2]{"DEFAULT"=>ModeKind::Normal,"BONUS"=>ModeKind::Purchase,_=>ModeKind::AnteBet}};if !found.contains(&k){found.push(k)}}}
    if found.len() == 1 { found.pop() } else { None }
}
fn alias(result: &mut BTreeMap<String, String>, name: &str, value: &str) -> Result<()> {
    if let Some(previous) = result.insert(name.into(), value.into()) {
        ensure!(previous == value, "GPAS conflicting source selectors");
    }
    Ok(())
}
pub(super) fn selectors(text: &str, costs: &[Value]) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    let names: BTreeSet<_> = costs.iter().filter_map(|r| r["name"].as_str()).collect();
    for c in re(r#"baseCostName:["']([^"']+)["'],(?:gpasFeatureType:["']([^"']+)["'],)?betParams:\{betType:[\w$.]+\.(DEFAULT|BONUS|EXTRA)"#).captures_iter(text){let name=if &c[1]=="default" {costs.first().and_then(|v|v["name"].as_str()).unwrap_or("default")}else{&c[1]};if names.contains(name){alias(&mut result,name,c.get(2).map(|v|v.as_str()).unwrap_or(&c[1]))?}}
    for c in re(r#"getContext\)?\([^;]{0,160}?contextNames\.default,\{transform:function\((\w+)\)\{return"([A-Za-z0-9_-]+)"\.concat\((\w+)\)\}\}"#).captures_iter(text){if c[1]==c[3]{for name in &names{alias(&mut result,name,&format!("{}{name}",&c[2]))?}}}
    if text.contains("SlotConfigurationParser") && text.contains("ReelsSpinEvent") {
        let native_line = re(r#"\(([\w$]+)="lines-"\+[\w$]+\([\w$.]+\)\.a\)\),([\w$]+)\}"#)
            .captures_iter(text)
            .any(|c| c[1] == c[2]);
        if native_line {
            let lines: BTreeSet<_> = costs
                .iter()
                .filter_map(|r| r["parameters"]["winline"].as_str())
                .collect();
            for row in costs {
                if let (Some(name), Some(line)) =
                    (row["name"].as_str(), row["parameters"]["winline"].as_str())
                {
                    let unique = costs
                        .iter()
                        .filter(|r| r["parameters"]["winline"] == line)
                        .count()
                        == 1;
                    if !line.is_empty()
                        && line.bytes().all(|c| c.is_ascii_digit())
                        && (row["parameters"]["startingMode"] == "Standard" || unique)
                    {
                        alias(
                            &mut result,
                            name,
                            &if lines.len() > 1 {
                                format!("lines-{line}")
                            } else {
                                "default".into()
                            },
                        )?
                    }
                }
            }
        }
    }
    // Pure map transformer, constrained to consumption by the default context.
    let mapper=re(r"prototype\.transform=function\((\w+)\)\{return this\.map\[(\w+)\]\?this\.map\[(\w+)\]:(\w+)\}").captures_iter(text).any(|c|c[1]==c[2]&&c[1]==c[3]&&c[1]==c[4]);
    if mapper {
        for c in re(r"(\w+)=new [\w$.]+\(\{([^{}]{1,1000})\}\)").captures_iter(text) {
            let end = c.get(0).unwrap().end();
            let mut tail_end = text.len().min(end + 700);
            while !text.is_char_boundary(tail_end) {
                tail_end -= 1;
            }
            let tail = &text[end..tail_end];
            let consumed = re(&format!(
                r"getContext\)?\([^;]{{0,180}}?contextNames\.default,{}\)",
                regex::escape(&c[1])
            ))
            .is_match(tail);
            if !consumed {
                continue;
            }
            let mut pairs = vec![];
            let mut valid = true;
            for field in c[2].split(',') {
                let Some(pair)=re(r#"^\s*(?:"([^"]+)"|([\w-]+))\s*:\s*(?:"([^"]+)"|[\w$]+\.betTypes\.(default|bonus))\s*$"#).captures(field)else{valid=false;break};
                let name = pair.get(1).or_else(|| pair.get(2)).unwrap().as_str();
                let value = pair.get(3).or_else(|| pair.get(4)).unwrap().as_str();
                if pair.get(3).is_none()
                    && !re(&format!(r#"betTypes=\{{[^}}]{{0,160}}\b{value}:"{value}""#))
                        .is_match(text)
                {
                    valid = false;
                    break;
                }
                pairs.push((name.to_owned(), value.to_owned()));
            }
            if valid {
                for (name, value) in pairs {
                    if names.contains(name.as_str()) {
                        alias(&mut result, &name, &value)?
                    }
                }
            }
        }
    }
    Ok(result)
}
