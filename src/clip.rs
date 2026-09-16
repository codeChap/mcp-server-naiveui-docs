//! Utf-8-safe string clipping. Never byte-slice serialized JSON.

use serde_json::{Map, Value};

pub const SNIPPET_LIMIT: usize = 280;
pub const BODY_LIMIT: usize = 24_000;
pub const JSON_CAP: usize = 48_000;

pub fn clip(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

pub fn clip_with_flag(s: &str, max_bytes: usize) -> (String, bool) {
    if s.len() <= max_bytes {
        (s.to_string(), false)
    } else {
        (clip(s, max_bytes), true)
    }
}

const CLIP_KEYS: &[&str] = &["description", "qa_markdown", "body", "markdown", "snippet"];
const OMIT_KEYS: &[&str] = &["extra_types", "qa_markdown", "alerts", "extra_sections"];

pub fn pretty_len(v: &Value) -> usize {
    serde_json::to_string_pretty(v)
        .map(|s| s.len())
        .unwrap_or(usize::MAX)
}

fn clip_object_strings(map: &mut Map<String, Value>, cap: usize) -> bool {
    let mut any = false;
    for key in CLIP_KEYS {
        if let Some(Value::String(s)) = map.get_mut(*key)
            && s.len() > cap
        {
            *s = clip(s, cap);
            any = true;
        }
    }
    any
}

fn omit_demo_titles(map: &mut Map<String, Value>) -> bool {
    let Some(Value::Array(demos)) = map.get_mut("demos") else {
        return false;
    };
    let mut any = false;
    for d in demos {
        if let Value::Object(o) = d
            && o.remove("title").is_some()
        {
            any = true;
        }
    }
    any
}

/// Drop optional fields until pretty JSON is ≤ `cap`. Never slices the serialized document.
pub fn omit_until_fits(mut value: Value, cap: usize) -> (Value, bool) {
    if pretty_len(&value) <= cap {
        return (value, false);
    }
    let mut truncated = false;
    if let Value::Object(map) = &mut value {
        truncated |= clip_object_strings(map, cap);
    }
    if truncated && pretty_len(&value) <= cap {
        return (value, true);
    }
    for key in OMIT_KEYS {
        let removed = match &mut value {
            Value::Object(map) => map.remove(*key).is_some(),
            _ => false,
        };
        if removed {
            truncated = true;
            if pretty_len(&value) <= cap {
                return (value, true);
            }
        }
    }
    let stripped_titles = match &mut value {
        Value::Object(map) => omit_demo_titles(map),
        _ => false,
    };
    if stripped_titles {
        truncated = true;
    }
    (value, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clip_short_unchanged() {
        assert_eq!(clip("hi", 10), "hi");
        assert_eq!(clip_with_flag("hi", 10), ("hi".into(), false));
    }

    #[test]
    fn clip_multibyte_stays_on_char_boundary() {
        let s = "é".repeat(50);
        let out = clip(&s, 7);
        assert!(out.is_char_boundary(out.len()));
        assert!(out.len() <= 7);
        assert!(clip_with_flag(&s, 7).1);
    }

    #[test]
    fn omit_until_fits_under_cap_unchanged() {
        let v = json!({"id": "button", "extra_types": [1]});
        let (out, truncated) = omit_until_fits(v.clone(), 10_000);
        assert_eq!(out, v);
        assert!(!truncated);
    }

    #[test]
    fn omit_until_fits_drops_extra_types_then_qa() {
        let v = json!({
            "id": "x",
            "extra_types": ["a".repeat(80)],
            "qa_markdown": "q".repeat(80),
        });
        let without_extra = json!({
            "id": "x",
            "qa_markdown": "q".repeat(80),
        });
        let cap = pretty_len(&without_extra) + 8;
        let (out, truncated) = omit_until_fits(v, cap);
        assert!(truncated);
        assert!(out.get("extra_types").is_none());
        let s = serde_json::to_string_pretty(&out).unwrap();
        assert!(s.len() <= cap);
        serde_json::from_str::<Value>(&s).unwrap();
    }

    #[test]
    fn omit_until_fits_never_byte_slices_json() {
        let v = json!({"id": "x", "apis": ["z".repeat(200)]});
        let (out, _) = omit_until_fits(v, 40);
        let s = serde_json::to_string_pretty(&out).unwrap();
        serde_json::from_str::<Value>(&s).expect("pretty JSON must remain parseable");
        assert!(out.get("id").is_some());
        assert!(out.get("apis").is_some());
    }

    #[test]
    fn omit_until_fits_strips_demo_titles() {
        let v = json!({
            "id": "button",
            "demos": [{"fence_id": "basic.vue", "title": "Basic".repeat(40)}],
        });
        let cap = 80;
        let (out, truncated) = omit_until_fits(v, cap);
        assert!(truncated);
        let title = out["demos"][0].get("title");
        assert!(title.is_none(), "{out}");
        serde_json::from_str::<Value>(&serde_json::to_string_pretty(&out).unwrap()).unwrap();
    }
}
