//! Walk the clone and build an in-memory catalog.
use std::collections::HashMap;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::Serialize;
use walkdir::{DirEntry, WalkDir};

use crate::clip::{SNIPPET_LIMIT, clip_with_flag};
use crate::names::{Names, Resolve, fold_ident, resolve};
use crate::parse::{ApiKind, Page, PageKind, extract_demo_title, parse_page};
use crate::sources::{REMOTE_ID, clone_dir, is_safe_rel};

pub(crate) const GOTCHAS_MD: &str = include_str!("../data/gotchas.md");
const CATEGORIES_JSON: &str = include_str!("../data/categories.json");

static PASCAL_EXPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(Nx?[A-Z][A-Za-z0-9]*)\b").expect("pascal export regex"));

const EXTRA_ALIASES: &[(&str, &str)] = &[
    ("button", "NxButton"),
    ("message", "useMessage"),
    ("message", "NMessageProvider"),
    ("dialog", "useDialog"),
    ("dialog", "NDialogProvider"),
    ("notification", "useNotification"),
    ("notification", "NNotificationProvider"),
    ("loading-bar", "useLoadingBar"),
    ("loading-bar", "NLoadingBarProvider"),
    ("modal", "useModal"),
    ("modal", "NModalProvider"),
    ("discrete", "createDiscreteApi"),
    ("docs/theme", "useThemeVars"),
    ("avatar", "avatar-group"),
    ("button", "button-group"),
    ("float-button", "float-button-group"),
];

#[derive(Debug, Clone)]
pub struct PropHit {
    pub page_id: String,
    pub heading: String,
    pub kind: ApiKind,
    pub row: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub score: u32,
    pub kind: PageKind,
    pub title: String,
    pub tag: Option<String>,
    pub pascal: Option<String>,
    pub site_url: String,
    pub snippet: String,
    #[serde(rename = "match")]
    pub match_kind: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListRow {
    pub id: String,
    pub tag: String,
    pub pascal: String,
    pub title: String,
    pub kind: PageKind,
    pub category: String,
    pub site_url: String,
    pub demo_count: usize,
    pub prop_count: usize,
}

#[derive(Debug)]
pub enum PageResolve<'a> {
    Hit {
        page: &'a Page,
        owner: Option<String>,
    },
    Candidates(Vec<String>),
    None {
        did_you_mean: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub pages: Vec<Page>,
    pub missing: Vec<String>,
    pub built_at: u64,
    pub by_id: HashMap<String, usize>,
    pub by_tag: HashMap<String, usize>,
    pub by_pascal: HashMap<String, usize>,
    pub props: HashMap<String, Vec<PropHit>>,
}

impl Catalog {
    pub fn load(cache: &Path) -> Self {
        let start = Instant::now();
        let mut pages = Vec::new();
        let mut missing = Vec::new();

        let mut gotchas = parse_page("gotchas", GOTCHAS_MD, "data/gotchas.md");
        gotchas.category = "Unlisted".into();
        pages.push(gotchas);

        let clone = clone_dir(cache);
        if !clone.is_dir() {
            missing.push(REMOTE_ID.to_string());
        } else {
            let categories = load_categories();
            walk_components(&clone, &categories, &mut pages);
            walk_docs(&clone, &mut pages);
        }

        for page in &mut pages {
            attach_owner_names(page);
            attach_extra_alias_names(page);
        }
        let mut catalog = Catalog {
            pages,
            missing,
            built_at: unix_now(),
            by_id: HashMap::new(),
            by_tag: HashMap::new(),
            by_pascal: HashMap::new(),
            props: HashMap::new(),
        };
        fill_indexes(&mut catalog);
        let elapsed = start.elapsed();
        let n = catalog.pages.len();
        tracing::debug!(?elapsed, pages = n, "catalog load");
        if elapsed.as_millis() > 500 {
            tracing::info!(?elapsed, pages = n, "catalog load > 500ms");
        }
        catalog
    }

    pub fn component_count(&self) -> usize {
        self.pages
            .iter()
            .filter(|p| !matches!(p.kind, PageKind::Doc | PageKind::Gotchas))
            .count()
    }

    pub fn demo_count(&self) -> usize {
        self.pages.iter().map(|p| p.demos.len()).sum()
    }

    pub fn doc_count(&self) -> usize {
        self.pages
            .iter()
            .filter(|p| p.kind == PageKind::Doc)
            .count()
    }

    pub fn gotchas_count(&self) -> usize {
        self.pages
            .iter()
            .filter(|p| p.kind == PageKind::Gotchas)
            .count()
    }

    pub fn get(&self, id: &str) -> Option<&Page> {
        let idx = self
            .by_id
            .get(id)
            .or_else(|| self.by_tag.get(id))
            .or_else(|| self.by_pascal.get(id))?;
        self.pages.get(*idx)
    }

    pub fn resolve_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.pages.iter().map(|p| p.id.clone()).collect();
        for page in &self.pages {
            for owner in &page.components {
                let n = Names::from_heading(owner);
                if n.kebab != "a" {
                    ids.push(n.kebab);
                }
            }
        }
        ids.sort();
        ids.dedup();
        ids
    }

    pub fn resolve_page(&self, name: &str) -> PageResolve<'_> {
        if let Some(page) = self.get(name) {
            return PageResolve::Hit {
                owner: owner_filter_for(page, name),
                page,
            };
        }
        let ids = self.resolve_ids();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        match resolve(name, &refs) {
            Resolve::Hit(id) => match self.get(&id) {
                Some(page) => PageResolve::Hit {
                    owner: owner_filter_for(page, name),
                    page,
                },
                None => PageResolve::None {
                    did_you_mean: vec![id],
                },
            },
            Resolve::Candidates(c) => PageResolve::Candidates(c),
            Resolve::None => PageResolve::None {
                did_you_mean: Vec::new(),
            },
        }
    }

    pub fn list_rows(
        &self,
        category: Option<&str>,
        kind: Option<PageKind>,
        query: Option<&str>,
    ) -> Vec<ListRow> {
        let q = query
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase);
        let mut rows: Vec<ListRow> = self
            .pages
            .iter()
            .filter(|p| kind.is_none_or(|k| p.kind == k))
            .filter(|p| category.is_none_or(|c| category_matches(&p.category, c)))
            .filter(|p| q.as_deref().is_none_or(|qq| list_query_hit(p, qq)))
            .map(list_row)
            .collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    pub fn search(
        &self,
        query: &str,
        kind: Option<PageKind>,
        limit: usize,
    ) -> (Vec<SearchHit>, bool) {
        let tokens: Vec<String> = query
            .split_whitespace()
            .map(str::to_lowercase)
            .filter(|t| t.len() > 1)
            .collect();
        if tokens.is_empty() {
            return (Vec::new(), false);
        }
        let q_debug = query.to_ascii_lowercase().contains("debug");
        let mut scored: Vec<SearchHit> = self
            .pages
            .iter()
            .filter(|p| kind.is_none_or(|k| p.kind == k))
            .filter_map(|p| score_page(p, &tokens, q_debug))
            .collect();
        // Prop-name hits outrank body/gotchas so `remote` surfaces data-table first.
        scored.sort_by(|a, b| {
            let ap = a.match_kind.starts_with("prop:");
            let bp = b.match_kind.starts_with("prop:");
            bp.cmp(&ap)
                .then_with(|| b.score.cmp(&a.score))
                .then_with(|| a.id.cmp(&b.id))
        });
        let truncated_hits = scored.len() > limit;
        scored.truncate(limit);
        (scored, truncated_hits)
    }

    pub fn lookup_props(&self, name: &str) -> Vec<&PropHit> {
        let raw = name.trim();
        if raw.is_empty() {
            return Vec::new();
        }
        let key = raw.to_ascii_lowercase();
        let folded = fold_prop(raw);
        let mut out: Vec<&PropHit> = Vec::new();
        if let Some(hits) = self.props.get(&key) {
            out.extend(hits);
        }
        for (k, hits) in &self.props {
            if k == &key {
                continue;
            }
            if fold_prop(k) == folded {
                out.extend(hits);
            }
        }
        out
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_categories() -> HashMap<String, String> {
    let v: serde_json::Value =
        serde_json::from_str(CATEGORIES_JSON).unwrap_or(serde_json::json!({}));
    let mut map = HashMap::new();
    let Some(obj) = v.as_object() else {
        return map;
    };
    for (k, val) in obj {
        if k.starts_with('_') {
            continue;
        }
        if let Some(s) = val.as_str() {
            map.insert(k.clone(), s.to_string());
        }
    }
    map
}

fn skip_walk(entry: &DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    matches!(name.as_ref(), ".git" | "target" | "node_modules" | "zhCN") || name.starts_with('_')
}

fn walk_components(clone: &Path, categories: &HashMap<String, String>, pages: &mut Vec<Page>) {
    let src_root = clone.join("src");
    if !src_root.is_dir() {
        return;
    }
    for ent in WalkDir::new(&src_root)
        .into_iter()
        .filter_entry(|e| !skip_walk(e))
        .filter_map(Result::ok)
    {
        let path = ent.path();
        if !is_en_demo_entry(path) {
            continue;
        }
        let Some(id) = component_id_from_demo_entry(path, clone) else {
            continue;
        };
        let source_path = rel_unix(path, clone);
        let md = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %path.display(), "skip unreadable page: {e}");
                continue;
            }
        };
        let mut page = parse_page(&id, &md, &source_path);
        page.category = categories
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "Unlisted".into());
        fill_demo_titles(&mut page, clone);
        attach_index_ts_pascals(&mut page, clone);
        pages.push(page);
    }
}

fn walk_docs(clone: &Path, pages: &mut Vec<Page>) {
    let docs_root = clone.join("demo").join("pages").join("docs");
    if !docs_root.is_dir() {
        return;
    }
    for ent in WalkDir::new(&docs_root)
        .into_iter()
        .filter_entry(|e| !skip_walk(e))
        .filter_map(Result::ok)
    {
        let path = ent.path();
        if !is_en_docs_page(path) {
            continue;
        }
        let Some(id) = docs_id_from_path(path, clone) else {
            continue;
        };
        let source_path = rel_unix(path, clone);
        let md = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %path.display(), "skip unreadable doc: {e}");
                continue;
            }
        };
        let mut page = parse_page(&id, &md, &source_path);
        page.category = "Unlisted".into();
        pages.push(page);
    }
}

fn is_en_demo_entry(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.ends_with("/demos/enUS/index.demo-entry.md")
}

fn is_en_docs_page(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name != "index.md" && name != "index.demo-entry.md" {
        return false;
    }
    path.components().any(|c| c.as_os_str() == "enUS")
}

fn component_id_from_demo_entry(path: &Path, clone: &Path) -> Option<String> {
    let rel = rel_unix(path, clone);
    let rest = rel.strip_prefix("src/")?;
    let id = rest.split('/').next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn docs_id_from_path(path: &Path, clone: &Path) -> Option<String> {
    let rel = rel_unix(path, clone);
    let rest = rel.strip_prefix("demo/pages/docs/")?;
    let slug = rest.split('/').next()?;
    if slug.is_empty() {
        None
    } else {
        Some(format!("docs/{slug}"))
    }
}

fn rel_unix(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn fill_demo_titles(page: &mut Page, clone: &Path) {
    let demos_dir = clone.join("src").join(&page.id).join("demos").join("enUS");
    for demo in &mut page.demos {
        if !is_safe_rel(&demo.file_name) {
            continue;
        }
        let path = demos_dir.join(&demo.file_name);
        match std::fs::read_to_string(&path) {
            Ok(src) => demo.title = extract_demo_title(&src),
            Err(_) => tracing::debug!(path = %path.display(), "missing demo file"),
        }
    }
}

fn pascals_from_index_ts(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for cap in PASCAL_EXPORT.captures_iter(src) {
        let name = cap[1].to_string();
        if !out.iter().any(|x| x == &name) {
            out.push(name);
        }
    }
    out
}

fn attach_index_ts_pascals(page: &mut Page, clone: &Path) {
    let index_ts = clone.join("src").join(&page.id).join("index.ts");
    let Ok(src) = std::fs::read_to_string(&index_ts) else {
        return;
    };
    for name in pascals_from_index_ts(&src) {
        push_unique(&mut page.pascals, name);
    }
}

fn attach_owner_names(page: &mut Page) {
    let owners: Vec<String> = page.components.clone();
    for owner in owners {
        let n = Names::from_heading(&owner);
        if n.kebab == "a" {
            continue;
        }
        push_unique(&mut page.tags, n.tag);
        push_unique(&mut page.pascals, n.pascal);
    }
}

fn attach_extra_alias_names(page: &mut Page) {
    for &(pid, alias) in EXTRA_ALIASES {
        if page.id != pid {
            continue;
        }
        if alias.starts_with("n-") {
            push_unique(&mut page.tags, alias.to_string());
        } else if alias.starts_with('N') {
            push_unique(&mut page.pascals, alias.to_string());
        }
    }
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.iter().any(|x| x == &s) {
        v.push(s);
    }
}

fn fill_indexes(catalog: &mut Catalog) {
    for idx in 0..catalog.pages.len() {
        let page_id = catalog.pages[idx].id.clone();
        catalog.by_id.insert(page_id, idx);
        let tags = catalog.pages[idx].tags.clone();
        for tag in tags {
            catalog.by_tag.insert(tag.clone(), idx);
            catalog.by_id.entry(tag).or_insert(idx);
        }
        let pascals = catalog.pages[idx].pascals.clone();
        for pascal in pascals {
            catalog.by_pascal.insert(pascal.clone(), idx);
            catalog.by_id.entry(pascal).or_insert(idx);
        }
        let owners = catalog.pages[idx].components.clone();
        for owner in owners {
            let n = Names::from_heading(&owner);
            if n.kebab == "a" {
                continue;
            }
            catalog.by_id.entry(n.kebab).or_insert(idx);
            catalog.by_id.entry(n.tag.clone()).or_insert(idx);
            catalog.by_id.entry(n.pascal.clone()).or_insert(idx);
            catalog.by_tag.entry(n.tag).or_insert(idx);
            catalog.by_pascal.entry(n.pascal).or_insert(idx);
        }
        let id = catalog.pages[idx].id.clone();
        for &(pid, alias) in EXTRA_ALIASES {
            if id != pid {
                continue;
            }
            catalog.by_id.entry(alias.to_string()).or_insert(idx);
            if alias.starts_with("n-") {
                catalog.by_tag.entry(alias.to_string()).or_insert(idx);
            } else if alias.starts_with('N') {
                catalog.by_pascal.entry(alias.to_string()).or_insert(idx);
            }
        }
        let hits: Vec<PropHit> = catalog.pages[idx]
            .apis
            .iter()
            .filter(|sec| matches!(sec.kind, ApiKind::Props | ApiKind::Properties))
            .flat_map(|sec| {
                sec.rows.iter().filter_map(|row| {
                    row.first()?;
                    Some(PropHit {
                        page_id: catalog.pages[idx].id.clone(),
                        heading: sec.heading.clone(),
                        kind: sec.kind,
                        row: row.clone(),
                    })
                })
            })
            .collect();
        for hit in hits {
            let key = hit
                .row
                .first()
                .map(|n| n.to_ascii_lowercase())
                .unwrap_or_default();
            catalog.props.entry(key).or_default().push(hit);
        }
    }
}

pub(crate) fn category_slug(group: &str) -> String {
    let lower = group.trim().to_ascii_lowercase();
    let stripped = lower
        .strip_suffix(" components")
        .unwrap_or(lower.as_str())
        .trim();
    stripped.replace(' ', "-")
}

fn category_matches(page_cat: &str, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    page_cat.eq_ignore_ascii_case(q) || category_slug(page_cat) == category_slug(q)
}

fn list_query_hit(page: &Page, q: &str) -> bool {
    page.id.to_ascii_lowercase().contains(q)
        || page.title.to_ascii_lowercase().contains(q)
        || page.tags.iter().any(|t| t.to_ascii_lowercase().contains(q))
}

fn list_row(page: &Page) -> ListRow {
    ListRow {
        id: page.id.clone(),
        tag: page.tags.first().cloned().unwrap_or_default(),
        pascal: page.pascals.first().cloned().unwrap_or_default(),
        title: page.title.clone(),
        kind: page.kind,
        category: page.category.clone(),
        site_url: page.site_url.clone(),
        demo_count: page.demos.iter().filter(|d| !d.debug).count(),
        prop_count: page
            .apis
            .iter()
            .filter(|s| matches!(s.kind, ApiKind::Props | ApiKind::Properties))
            .map(|s| s.rows.len())
            .sum(),
    }
}

fn owner_filter_for(page: &Page, query: &str) -> Option<String> {
    let q = fold_ident(query);
    if q.is_empty() {
        return None;
    }
    let names = Names::from_kebab(&page.id);
    if q == fold_ident(&page.id) || q == fold_ident(&names.tag) || q == fold_ident(&names.pascal) {
        return None;
    }
    page.components
        .iter()
        .find(|owner| {
            let n = Names::from_heading(owner);
            fold_ident(owner) == q
                || fold_ident(&n.kebab) == q
                || fold_ident(&n.tag) == q
                || fold_ident(&n.pascal) == q
        })
        .cloned()
}

fn fold_prop(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '-' && *c != '_')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn page_search_body(page: &Page) -> String {
    let mut s = page.description.clone();
    for a in &page.alerts {
        s.push('\n');
        s.push_str(a);
    }
    if let Some(qa) = &page.qa_markdown {
        s.push('\n');
        s.push_str(qa);
    }
    for e in &page.extra_sections {
        s.push('\n');
        s.push_str(&e.heading);
        s.push('\n');
        s.push_str(&e.markdown);
    }
    s
}

fn score_page(page: &Page, tokens: &[String], q_debug: bool) -> Option<SearchHit> {
    let mut score = 0u32;
    let mut match_kind = String::new();
    let mut snippet = String::new();
    let mut from_prop = false;

    let title = page.title.to_lowercase();
    let id = page.id.to_lowercase();
    let stem = id.replace('-', " ");
    let category = page.category.to_lowercase();
    let tags: Vec<String> = page.tags.iter().map(|t| t.to_lowercase()).collect();
    let pascals: Vec<String> = page.pascals.iter().map(|t| t.to_lowercase()).collect();

    for t in tokens {
        if title.contains(t) {
            score += 12;
            set_if_empty(&mut match_kind, "title");
        }
        if id.contains(t) {
            score += 10;
            set_if_empty(&mut match_kind, "id");
        }
        if tags.iter().any(|x| x.contains(t)) {
            score += 10;
            set_if_empty(&mut match_kind, "tag");
        }
        if pascals.iter().any(|x| x.contains(t)) {
            score += 10;
            set_if_empty(&mut match_kind, "pascal");
        }
        if stem.contains(t) {
            score += 10;
            set_if_empty(&mut match_kind, "stem");
        }
        if category.contains(t) {
            score += 4;
            set_if_empty(&mut match_kind, "category");
        }
    }

    let body = page_search_body(page);
    let body_lc = body.to_lowercase();
    let mut desc_matches = 0u32;
    for t in tokens {
        desc_matches += body_lc.matches(t.as_str()).count() as u32;
    }
    if desc_matches > 0 {
        score += desc_matches.min(4);
        set_if_empty(&mut match_kind, "description");
    }

    let mut type_desc = 0u32;
    for sec in &page.apis {
        if !matches!(sec.kind, ApiKind::Props | ApiKind::Properties) {
            continue;
        }
        for row in &sec.rows {
            let name = row.first().map(String::as_str).unwrap_or("");
            let name_lc = name.to_ascii_lowercase();
            let name_fold = fold_prop(name);
            if tokens
                .iter()
                .any(|t| name_lc == *t || name_fold == fold_prop(t))
            {
                score += 14;
                match_kind = format!("prop:{name}");
                snippet = row.join(" | ");
                from_prop = true;
            }
            if row.len() > 1 {
                let rest = row[1..].join(" ").to_lowercase();
                for t in tokens {
                    if rest.contains(t) {
                        type_desc += 1;
                    }
                }
            }
        }
    }
    score += type_desc.min(3);

    for demo in &page.demos {
        if demo.debug && !q_debug {
            continue;
        }
        let file = demo.file_name.to_lowercase();
        let title_d = demo.title.as_deref().unwrap_or("").to_lowercase();
        for t in tokens {
            if file.contains(t) || title_d.contains(t) {
                score += 6;
                if !from_prop {
                    match_kind = format!("demo:{}", demo.file_name);
                }
            }
        }
    }

    if score == 0 {
        return None;
    }
    if page.kind == PageKind::Gotchas {
        score += 50;
    }
    if page.kind == PageKind::Api {
        score += 8;
    }

    let (snippet, truncated) = if from_prop {
        clip_with_flag(&snippet, SNIPPET_LIMIT)
    } else {
        let (sn, tr) = snippet_from_body(&body, tokens);
        if sn.is_empty() {
            clip_with_flag(&page.description, SNIPPET_LIMIT)
        } else {
            (sn, tr)
        }
    };

    Some(SearchHit {
        id: page.id.clone(),
        score,
        kind: page.kind,
        title: page.title.clone(),
        tag: page.tags.first().cloned(),
        pascal: page.pascals.first().cloned(),
        site_url: page.site_url.clone(),
        snippet,
        match_kind,
        truncated,
    })
}

fn set_if_empty(slot: &mut String, v: &str) {
    if slot.is_empty() {
        slot.push_str(v);
    }
}

fn snippet_from_body(body: &str, tokens: &[String]) -> (String, bool) {
    let line = body.lines().find(|l| {
        let lc = l.to_lowercase();
        tokens.iter().any(|t| lc.contains(t))
    });
    match line {
        Some(l) => clip_with_flag(l.trim(), SNIPPET_LIMIT),
        None => (String::new(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_cache() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tree")
    }

    #[test]
    fn load_fixture_tree() {
        let cat = Catalog::load(&fixture_cache());
        assert!(cat.missing.is_empty(), "{:?}", cat.missing);
        assert_eq!(cat.gotchas_count(), 1);
        assert!(cat.get("gotchas").is_some());
        assert!(cat.get("button").is_some());
        assert!(cat.get("n-button").is_some());
        assert!(cat.get("NButton").is_some());
        assert!(cat.get("NxButton").is_some());
        assert!(cat.get("data-table").is_some());
        assert!(cat.get("config-consumer").is_some());
        let button = cat.get("button").unwrap();
        assert_eq!(button.category, "Common Components");
        assert!(button.pascals.iter().any(|p| p == "NxButton"));
        let table = cat.get("data-table").unwrap();
        assert_eq!(table.category, "Data Display Components");
        let cc = cat.get("config-consumer").unwrap();
        assert_eq!(cc.category, "Unlisted");
        assert!(
            cc.demos.iter().any(|d| d.file_name == "basic.demo.md"),
            "missing-demo fence still catalogs"
        );
        assert_eq!(cat.component_count(), 4);
        assert_eq!(cat.doc_count(), 1);
        assert!(cat.pages.len() >= 6);
        assert!(cat.get("discrete").is_some());
        assert!(cat.get("_styles").is_none());
        assert!(cat.get("docs/customize-theme").is_some());
        let basic = button
            .demos
            .iter()
            .find(|d| d.file_name == "basic.demo.vue")
            .expect("basic demo");
        assert_eq!(basic.title.as_deref(), Some("Basic"));
        assert!(cat.props.contains_key("attr-type"));
        assert_eq!(category_slug("Data Display Components"), "data-display");
        assert_eq!(category_slug("Common Components"), "common");
    }

    #[test]
    fn missing_clone_still_has_gotchas() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("empty-cache-pr3");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cat = Catalog::load(&dir);
        assert_eq!(cat.missing, vec!["naive-ui"]);
        assert_eq!(cat.gotchas_count(), 1);
        assert_eq!(cat.pages.len(), 1);
        assert_eq!(cat.component_count(), 0);
    }

    #[test]
    fn skip_walk_private_and_zh() {
        let cat = Catalog::load(&fixture_cache());
        for p in &cat.pages {
            assert!(!p.source_path.contains("zhCN"), "{}", p.source_path);
            assert!(!p.id.starts_with('_'), "{}", p.id);
        }
    }

    #[test]
    fn categories_has_94_menu_paths() {
        let map = load_categories();
        assert_eq!(map.len(), 94, "94 menu paths at v2.40.4");
        assert!(!map.contains_key("config-consumer"));
        assert_eq!(
            map.get("button").map(String::as_str),
            Some("Common Components")
        );
    }

    #[test]
    fn search_remote_hits_data_table_first() {
        let cat = Catalog::load(&fixture_cache());
        let (hits, _) = cat.search("remote", None, 8);
        assert!(!hits.is_empty(), "expected hits for remote");
        assert_eq!(hits[0].id, "data-table");
        assert!(
            hits[0].match_kind.starts_with("prop:"),
            "{}",
            hits[0].match_kind
        );
        assert!(hits[0].snippet.contains("remote"), "{}", hits[0].snippet);
    }

    #[test]
    fn lookup_props_kebab_and_camel() {
        let cat = Catalog::load(&fixture_cache());
        let remote = cat.lookup_props("remote");
        assert!(
            remote.iter().any(|h| h.page_id == "data-table"),
            "remote prop"
        );
        let kebab = cat.lookup_props("row-key");
        let camel = cat.lookup_props("rowKey");
        assert!(!kebab.is_empty());
        assert_eq!(kebab.len(), camel.len());
        assert!(kebab.iter().any(|h| h.page_id == "data-table"));
    }

    #[test]
    fn resolve_n_datatable_and_list_button() {
        let cat = Catalog::load(&fixture_cache());
        match cat.resolve_page("n-datatable") {
            PageResolve::Hit { page, .. } => assert_eq!(page.id, "data-table"),
            other => panic!("expected data-table, got {other:?}"),
        }
        let rows = cat.list_rows(None, None, Some("button"));
        assert!(rows.iter().any(|r| r.id == "button"), "{rows:?}");
        let display = cat.list_rows(Some("data-display"), None, None);
        assert!(display.iter().any(|r| r.id == "data-table"));
        assert!(!display.iter().any(|r| r.id == "button"));
    }
}
