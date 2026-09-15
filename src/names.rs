//! Kebab / tag / Pascal mapping and in-tree Levenshtein did-you-mean.
#![allow(dead_code)]

/// Id forms derived from a kebab directory name or a heading owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names {
    pub kebab: String,
    pub tag: String,
    pub pascal: String,
}

impl Names {
    pub fn from_kebab(kebab: &str) -> Self {
        let kebab = kebab.trim().to_string();
        Self {
            tag: tag_from_kebab(&kebab),
            pascal: pascal_from_kebab(&kebab),
            kebab,
        }
    }

    pub fn from_heading(heading: &str) -> Self {
        Self::from_kebab(&kebab_from_heading(heading))
    }
}

pub fn tag_from_kebab(kebab: &str) -> String {
    format!("n-{kebab}")
}

/// `N` + each hyphen-separated segment capitalized (`data-table` → `NDataTable`).
pub fn pascal_from_kebab(kebab: &str) -> String {
    let mut out = String::from("N");
    for seg in kebab.split('-').filter(|s| !s.is_empty()) {
        let mut chars = seg.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Heading owner → kebab. `Layout Content` → `layout-content`; `ButtonGroup` → `button-group`.
pub fn kebab_from_heading(heading: &str) -> String {
    heading
        .split_whitespace()
        .map(pascal_to_kebab)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn pascal_to_kebab(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '-' {
            if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
            continue;
        }
        if c.is_ascii_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            let split = prev.is_ascii_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_ascii_uppercase() && next_lower);
            if split && !out.ends_with('-') {
                out.push('-');
            }
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// Unique resolution or a candidate list. Never guess when several ids match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolve {
    Hit(String),
    Candidates(Vec<String>),
    None,
}

impl Resolve {
    pub fn id(&self) -> Option<&str> {
        match self {
            Resolve::Hit(id) => Some(id),
            Resolve::Candidates(_) | Resolve::None => None,
        }
    }
}

/// Strip `n-` / `N` prefix, lowercase, collapse `[-_ ]` to hyphens, then
/// (a) exact kebab, (b) exact pascal, (c) unique prefix, (d) Levenshtein ≤ 2.
pub fn resolve(query: &str, kebab_ids: &[&str]) -> Resolve {
    let raw = query.trim();
    if raw.is_empty() || kebab_ids.is_empty() {
        return Resolve::None;
    }
    let names: Vec<Names> = kebab_ids.iter().map(|k| Names::from_kebab(k)).collect();

    let exact: Vec<&Names> = names
        .iter()
        .filter(|n| n.kebab == raw || n.tag == raw || n.pascal == raw)
        .collect();
    if let Some(r) = unique_or_ambiguous(&exact) {
        return r;
    }

    let kebab_q = normalize_to_kebab(raw);

    let a: Vec<&Names> = names.iter().filter(|n| n.kebab == kebab_q).collect();
    if let Some(r) = unique_or_ambiguous(&a) {
        return r;
    }

    let b: Vec<&Names> = names.iter().filter(|n| pascal_eq(raw, &n.pascal)).collect();
    if let Some(r) = unique_or_ambiguous(&b) {
        return r;
    }

    if !kebab_q.is_empty() {
        let c: Vec<&Names> = names
            .iter()
            .filter(|n| n.kebab.starts_with(&kebab_q))
            .collect();
        if let Some(r) = unique_or_ambiguous(&c) {
            return r;
        }
    }

    let mut scored: Vec<(usize, &Names)> = names
        .iter()
        .map(|n| (levenshtein(&kebab_q, &n.kebab), n))
        .filter(|(d, _)| *d <= 2)
        .collect();
    if scored.is_empty() {
        return Resolve::None;
    }
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.kebab.cmp(&b.1.kebab)));
    let best = scored[0].0;
    let top: Vec<&Names> = scored
        .iter()
        .filter(|(d, _)| *d == best)
        .map(|(_, n)| *n)
        .collect();
    unique_or_ambiguous(&top).unwrap_or(Resolve::None)
}

fn unique_or_ambiguous(hits: &[&Names]) -> Option<Resolve> {
    match hits.len() {
        0 => None,
        1 => Some(Resolve::Hit(hits[0].kebab.clone())),
        _ => {
            let mut ids: Vec<String> = hits.iter().map(|n| n.kebab.clone()).collect();
            ids.sort();
            ids.dedup();
            if ids.len() == 1 {
                Some(Resolve::Hit(ids.remove(0)))
            } else {
                Some(Resolve::Candidates(ids))
            }
        }
    }
}

fn pascal_eq(query: &str, pascal: &str) -> bool {
    if pascal.eq_ignore_ascii_case(query) {
        return true;
    }
    pascal
        .strip_prefix('N')
        .is_some_and(|rest| rest.eq_ignore_ascii_case(query))
}

fn strip_n_prefix(s: &str) -> &str {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return s;
    };
    if first != 'n' && first != 'N' {
        return s;
    }
    match chars.next() {
        Some('-' | '_' | ' ') => &s[first.len_utf8() + 1..],
        Some(c) if first == 'N' && (c.is_ascii_uppercase() || c.is_ascii_digit()) => {
            &s[first.len_utf8()..]
        }
        _ => s,
    }
}

fn normalize_to_kebab(raw: &str) -> String {
    let stripped = strip_n_prefix(raw.trim());
    let mut out = String::new();
    for c in stripped.chars() {
        let c = c.to_ascii_lowercase();
        if c == '_' || c == ' ' || c == '-' {
            if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.push(c);
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Classic Wagner–Fischer. No extra crate.
pub fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.chars().count();
    }
    if b.is_empty() {
        return a.chars().count();
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}
