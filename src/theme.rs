//! CSS vars from `*.cssr.ts` comments + common keys/literals from `_styles/common`.
//! Identifier/call RHS is stored verbatim — no JS evaluator, so `primaryColor` is not `#18a058`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};
use walkdir::WalkDir;

use crate::clip::{JSON_CAP, pretty_len};
use crate::names::{Names, kebab_from_heading};
use crate::sources::is_safe_source_id;

static CSS_VAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*//\s*(--n-[A-Za-z0-9-]+)").expect("css var regex"));

pub fn theme_unfiltered(clone: &Path) -> Value {
    let common = load_common(clone);
    let components = load_all_component_vars(clone);
    unfiltered_envelope(common, components)
}

pub fn theme_filtered(clone: &Path, id: &str) -> Value {
    let (css_vars, source_paths) = if is_safe_source_id(id) {
        load_component_vars(clone, id)
    } else {
        (Vec::new(), Vec::new())
    };
    json!({
        "id": id,
        "css_vars": css_vars,
        "source_paths": source_paths,
        "truncated": false,
    })
}

/// Guess a kebab dir from a tag / Pascal / kebab query when catalog has no page
/// (`avatar-group` has cssr but no English demo-entry).
pub fn guess_component_id(name: &str) -> String {
    let t = name.trim();
    if let Some(rest) = t.strip_prefix("n-").or_else(|| t.strip_prefix("N-")) {
        return rest.to_string();
    }
    let bytes = t.as_bytes();
    if bytes.first() == Some(&b'N') && bytes.get(1).is_some_and(u8::is_ascii_uppercase) {
        return kebab_from_heading(&t[1..]);
    }
    Names::from_heading(t).kebab
}

fn unfiltered_envelope(
    common: Map<String, Value>,
    components: BTreeMap<String, Vec<String>>,
) -> Value {
    let full = json!({
        "common": common,
        "components": components,
        "truncated": false,
    });
    if pretty_len(&full) <= JSON_CAP {
        return full;
    }
    // Unfiltered cssr lists blow the cap; counts stay valid JSON. Pass component=.
    let counts: BTreeMap<String, usize> = components
        .iter()
        .map(|(k, v)| (k.clone(), v.len()))
        .collect();
    json!({
        "common": common,
        "component_var_counts": counts,
        "truncated": true,
        "hint": "unfiltered theme JSON exceeded cap; pass component= for one css_vars list",
    })
}

fn load_common(clone: &Path) -> Map<String, Value> {
    let common_dir = clone.join("src").join("_styles").join("common");
    let mut out = Map::new();
    let common_ts = read_to_string(&common_dir.join("_common.ts"));
    for (k, v) in parse_export_default_object(&common_ts) {
        out.insert(k, v);
    }
    let light = read_to_string(&common_dir.join("light.ts"));
    for (k, v) in parse_derived_object(&light) {
        out.insert(k, v);
    }
    out
}

fn load_all_component_vars(clone: &Path) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for id in component_dirs(clone) {
        if !is_safe_source_id(&id) {
            continue;
        }
        let (vars, _) = load_component_vars(clone, &id);
        if !vars.is_empty() {
            out.insert(id, vars);
        }
    }
    out
}

fn load_component_vars(clone: &Path, id: &str) -> (Vec<String>, Vec<String>) {
    let dir = clone.join("src").join(id);
    if !dir.is_dir() {
        return (Vec::new(), Vec::new());
    }
    let mut files: Vec<PathBuf> = WalkDir::new(&dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".cssr.ts"))
        })
        .collect();
    files.sort();
    let mut vars = Vec::new();
    let mut paths = Vec::new();
    for path in files {
        let rel = rel_unix(&path, clone);
        paths.push(rel);
        let src = read_to_string(&path);
        for v in css_vars_from_cssr(&src) {
            if !vars.iter().any(|x| x == &v) {
                vars.push(v);
            }
        }
    }
    (vars, paths)
}

fn component_dirs(clone: &Path) -> Vec<String> {
    let src = clone.join("src");
    let mut ids = Vec::new();
    let Ok(rd) = std::fs::read_dir(&src) else {
        return ids;
    };
    for ent in rd.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('_') || name.starts_with('.') {
            continue;
        }
        if ent.path().is_dir() {
            ids.push(name.into_owned());
        }
    }
    ids.sort();
    ids
}

fn css_vars_from_cssr(src: &str) -> Vec<String> {
    let mut skip_private = false;
    let mut out = Vec::new();
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("//")
            && t.trim_start_matches('/')
                .trim_start()
                .starts_with("private-vars:")
        {
            skip_private = true;
            continue;
        }
        if skip_private {
            if t.is_empty() || t.starts_with("//") {
                continue;
            }
            skip_private = false;
        }
        let Some(cap) = CSS_VAR.captures(line) else {
            continue;
        };
        let var = &cap[1];
        if var.contains("xxx") || line.contains("xxx") {
            continue;
        }
        if !out.iter().any(|x| x == var) {
            out.push(var.to_string());
        }
    }
    out
}

fn parse_derived_object(src: &str) -> Vec<(String, Value)> {
    object_props_after(src, "const derived")
}

fn parse_export_default_object(src: &str) -> Vec<(String, Value)> {
    object_props_after(src, "export default")
}

fn object_props_after(src: &str, marker: &str) -> Vec<(String, Value)> {
    let Some(body) = object_body_after(src, marker) else {
        return Vec::new();
    };
    parse_object_props(body)
}

fn object_body_after<'a>(src: &'a str, marker: &str) -> Option<&'a str> {
    let start = src.find(marker)?;
    let after = &src[start + marker.len()..];
    let rel = after.find('{')?;
    let open = start + marker.len() + rel;
    let close = find_matching_brace(src, open)?;
    src.get(open + 1..close)
}

fn find_matching_brace(src: &str, open: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if bytes.get(open) != Some(&b'{') {
        return None;
    }
    let mut depth = 0i32;
    let mut i = open;
    let mut in_str: Option<u8> = None;
    let mut escape = false;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' | b'`' => in_str = Some(b),
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = i.saturating_add(2);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_object_props(body: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < body.len() {
        i = skip_trivia(body, i);
        if i >= body.len() {
            break;
        }
        let rest = &body[i..];
        if rest.starts_with('}') {
            break;
        }
        if rest.starts_with("...") {
            i += 3;
            i = skip_ident(body, i);
            i = skip_trivia(body, i);
            if body.get(i..).is_some_and(|s| s.starts_with(',')) {
                i += 1;
            }
            continue;
        }
        let Some((key, next)) = parse_key(body, i) else {
            break;
        };
        i = skip_trivia(body, next);
        if !body.get(i..).is_some_and(|s| s.starts_with(':')) {
            break;
        }
        i += 1;
        i = skip_trivia(body, i);
        let (val, next) = parse_rhs(body, i);
        out.push((key, val));
        i = skip_trivia(body, next);
        if body.get(i..).is_some_and(|s| s.starts_with(',')) {
            i += 1;
        }
    }
    out
}

fn parse_key(s: &str, i: usize) -> Option<(String, usize)> {
    let rest = s.get(i..)?;
    if rest.starts_with('\'') || rest.starts_with('"') {
        let (k, end) = parse_quoted(s, i)?;
        return Some((k, end));
    }
    let end = skip_ident(s, i);
    if end == i {
        return None;
    }
    Some((s[i..end].to_string(), end))
}

fn parse_rhs(s: &str, i: usize) -> (Value, usize) {
    if i >= s.len() {
        return (json!({ "rhs": "" }), i);
    }
    let b = s.as_bytes()[i];
    if (b == b'\'' || b == b'"')
        && let Some((lit, mut end)) = parse_quoted(s, i)
    {
        end = skip_as_clause(s, end);
        return (json!({ "value": lit }), end);
    }
    if (b == b'-' || b.is_ascii_digit())
        && let Some((num, end)) = parse_number(s, i)
    {
        return (json!({ "value": num }), end);
    }
    let (rhs, end) = parse_other_rhs(s, i);
    (json!({ "rhs": rhs }), end)
}

fn parse_quoted(s: &str, start: usize) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    let q = *bytes.get(start)?;
    if q != b'\'' && q != b'"' {
        return None;
    }
    let mut out = String::new();
    let mut i = start + 1;
    let mut esc = false;
    while i < bytes.len() {
        let b = bytes[i];
        if esc {
            match b {
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                _ => out.push(b as char),
            }
            esc = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            esc = true;
            i += 1;
            continue;
        }
        if b == q {
            return Some((out, i + 1));
        }
        let ch = s[i..].chars().next()?;
        out.push(ch);
        i += ch.len_utf8();
    }
    None
}

fn parse_number(s: &str, i: usize) -> Option<(serde_json::Number, usize)> {
    let rest = s.get(i..)?;
    let mut end = 0;
    let bytes = rest.as_bytes();
    if bytes.first() == Some(&b'-') {
        end = 1;
    }
    let start_digits = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    if end == start_digits || (bytes.get(start_digits) == Some(&b'.') && end == start_digits + 1) {
        return None;
    }
    let raw = &rest[..end];
    let n: serde_json::Number = raw.parse().ok()?;
    Some((n, i + end))
}

fn parse_other_rhs(s: &str, start: usize) -> (String, usize) {
    let bytes = s.as_bytes();
    let mut i = start;
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_brack = 0i32;
    let mut in_str: Option<u8> = None;
    let mut escape = false;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' | b'`' => in_str = Some(b),
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'{' => depth_brace += 1,
            b'}' => {
                if depth_brace == 0 && depth_paren == 0 && depth_brack == 0 {
                    break;
                }
                depth_brace -= 1;
            }
            b'[' => depth_brack += 1,
            b']' => depth_brack -= 1,
            b',' if depth_paren == 0 && depth_brace == 0 && depth_brack == 0 => break,
            b'\n' if depth_paren == 0 && depth_brace == 0 && depth_brack == 0 => break,
            b'/' if depth_paren == 0
                && depth_brace == 0
                && depth_brack == 0
                && bytes.get(i + 1) == Some(&b'/') =>
            {
                break;
            }
            _ => {}
        }
        i += 1;
    }
    (s[start..i].trim().to_string(), i)
}

fn skip_as_clause(s: &str, i: usize) -> usize {
    let j = skip_trivia(s, i);
    let rest = s.get(j..).unwrap_or("");
    if rest.starts_with("as ") || rest.starts_with("as\t") {
        skip_ident(s, skip_trivia(s, j + 2))
    } else {
        i
    }
}

fn skip_trivia(s: &str, mut i: usize) -> usize {
    let bytes = s.as_bytes();
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if s.get(i..).is_some_and(|r| r.starts_with("//")) {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if s.get(i..).is_some_and(|r| r.starts_with("/*")) {
            if let Some(rel) = s[i + 2..].find("*/") {
                i += 2 + rel + 2;
                continue;
            }
            return s.len();
        }
        return i;
    }
}

fn skip_ident(s: &str, i: usize) -> usize {
    let bytes = s.as_bytes();
    let Some(&b) = bytes.get(i) else {
        return i;
    };
    if !b.is_ascii_alphabetic() && b != b'_' && b != b'$' {
        return i;
    }
    let mut j = i + 1;
    while j < bytes.len()
        && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'$')
    {
        j += 1;
    }
    j
}

fn read_to_string(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn rel_unix(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::clone_dir;
    use std::path::PathBuf;

    fn fixture_clone() -> PathBuf {
        clone_dir(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tree"))
    }

    #[test]
    fn button_cssr_has_text_color_not_xxx() {
        let clone = fixture_clone();
        let v = theme_filtered(&clone, "button");
        let vars = v["css_vars"].as_array().unwrap();
        let names: Vec<&str> = vars.iter().filter_map(|x| x.as_str()).collect();
        assert!(
            names.contains(&"--n-text-color"),
            "expected --n-text-color in {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("xxx")),
            "xxx vars must be skipped: {names:?}"
        );
        assert!(!names.contains(&"--n-border-color-xxx"));
        let paths = v["source_paths"].as_array().unwrap();
        assert!(
            paths
                .iter()
                .any(|p| p.as_str().unwrap().ends_with("index.cssr.ts")),
            "{paths:?}"
        );
        assert_eq!(v["id"], "button");
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn glob_includes_non_index_cssr() {
        let clone = fixture_clone();
        let v = theme_filtered(&clone, "avatar-group");
        let vars = v["css_vars"].as_array().unwrap();
        let names: Vec<&str> = vars.iter().filter_map(|x| x.as_str()).collect();
        assert!(
            names.contains(&"--n-gap"),
            "avatar-group.cssr.ts (not index.cssr.ts) must be globbed: {names:?}"
        );
        let paths = v["source_paths"].as_array().unwrap();
        assert!(
            paths
                .iter()
                .any(|p| p.as_str().unwrap().ends_with("avatar-group.cssr.ts")),
            "{paths:?}"
        );
    }

    #[test]
    fn light_ts_literal_vs_identifier() {
        let clone = fixture_clone();
        let v = theme_unfiltered(&clone);
        let common = &v["common"];
        assert_eq!(common["textColor1"]["value"], "rgb(31, 34, 37)");
        assert!(common["textColor1"].get("rhs").is_none());
        assert_eq!(common["primaryColor"]["rhs"], "base.primaryDefault");
        assert!(
            common["primaryColor"].get("value").is_none(),
            "do not promise a resolved primaryColor: {}",
            common["primaryColor"]
        );
        let dump = serde_json::to_string(common).unwrap();
        assert!(
            !dump.contains("#18a058"),
            "must not leak base.primaryDefault hex into common: {dump}"
        );
        let font = common["fontFamily"]["value"].as_str().unwrap();
        assert!(font.contains("v-sans"), "{font}");
        assert_eq!(v["truncated"], false);
        assert!(
            v["components"]["button"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x.as_str() == Some("--n-text-color"))
        );
        serde_json::from_str::<Value>(&serde_json::to_string_pretty(&v).unwrap()).unwrap();
    }

    #[test]
    fn missing_styles_dir_is_empty_not_error() {
        let clone = fixture_clone();
        let v = theme_filtered(&clone, "no-such-component");
        assert_eq!(v["id"], "no-such-component");
        assert_eq!(v["css_vars"], json!([]));
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn private_vars_block_skipped_even_without_xxx() {
        let src = r#"
// --n-public
// private-vars:
// --n-secret
export default c([])
"#;
        let vars = css_vars_from_cssr(src);
        assert_eq!(vars, vec!["--n-public".to_string()]);
    }

    #[test]
    fn derived_parser_literals_and_calls() {
        let src = r#"
const derived = {
  name: 'common' as const,
  ...commonVariables,
  primaryColor: base.primaryDefault,
  textColor1: 'rgb(31, 34, 37)',
  iconColorHover: scaleColor(neutral(base.alpha4), { lightness: 0.75 }),
  count: 12,
}
"#;
        let props: Map<String, Value> = parse_derived_object(src).into_iter().collect();
        assert_eq!(props["name"]["value"], "common");
        assert_eq!(props["primaryColor"]["rhs"], "base.primaryDefault");
        assert!(props["primaryColor"].get("value").is_none());
        assert_eq!(props["textColor1"]["value"], "rgb(31, 34, 37)");
        assert_eq!(
            props["iconColorHover"]["rhs"],
            "scaleColor(neutral(base.alpha4), { lightness: 0.75 })"
        );
        assert_eq!(props["count"]["value"], 12);
        assert!(!props.contains_key("commonVariables"));
    }

    #[test]
    fn unfiltered_overflow_omits_arrays_keeps_valid_json() {
        let mut common = Map::new();
        common.insert("textColor1".into(), json!({ "value": "rgb(31, 34, 37)" }));
        let mut components = BTreeMap::new();
        for i in 0..800 {
            let vars: Vec<String> = (0..20).map(|j| format!("--n-var-{j:02}")).collect();
            components.insert(format!("comp-{i:03}"), vars);
        }
        let full = json!({
            "common": common,
            "components": components,
            "truncated": false,
        });
        assert!(pretty_len(&full) > JSON_CAP);
        let v = unfiltered_envelope(common, components);
        assert_eq!(v["truncated"], true);
        assert!(v.get("components").is_none(), "{v}");
        assert_eq!(v["component_var_counts"]["comp-000"], 20);
        assert_eq!(v["common"]["textColor1"]["value"], "rgb(31, 34, 37)");
        let s = serde_json::to_string_pretty(&v).unwrap();
        serde_json::from_str::<Value>(&s).expect("overflow payload must stay parseable");
        assert!(s.len() <= JSON_CAP, "{} > {JSON_CAP}", s.len());
    }

    #[test]
    fn guess_nbutton_and_tag() {
        assert_eq!(guess_component_id("n-button"), "button");
        assert_eq!(guess_component_id("NButton"), "button");
        assert_eq!(guess_component_id("button"), "button");
        assert_eq!(guess_component_id("avatar-group"), "avatar-group");
    }
}
