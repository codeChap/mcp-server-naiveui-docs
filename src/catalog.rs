//! Walk the clone and build an in-memory catalog.
use std::collections::HashMap;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use walkdir::{DirEntry, WalkDir};

use crate::names::Names;
use crate::parse::{ApiKind, Page, PageKind, extract_demo_title, parse_page};
use crate::sources::{REMOTE_ID, clone_dir, is_safe_rel};

const GOTCHAS_MD: &str = include_str!("../data/gotchas.md");
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
#[allow(dead_code)]
pub struct PropHit {
    pub page_id: String,
    pub heading: String,
    pub kind: ApiKind,
    pub row: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub pages: Vec<Page>,
    pub missing: Vec<String>,
    pub built_at: u64,
    #[allow(dead_code)]
    pub by_id: HashMap<String, usize>,
    #[allow(dead_code)]
    pub by_tag: HashMap<String, usize>,
    #[allow(dead_code)]
    pub by_pascal: HashMap<String, usize>,
    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub fn get(&self, id: &str) -> Option<&Page> {
        let idx = self
            .by_id
            .get(id)
            .or_else(|| self.by_tag.get(id))
            .or_else(|| self.by_pascal.get(id))?;
        self.pages.get(*idx)
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

#[cfg(test)]
fn category_slug(group: &str) -> String {
    let lower = group.trim().to_ascii_lowercase();
    let stripped = lower
        .strip_suffix(" components")
        .unwrap_or(lower.as_str())
        .trim();
    stripped.replace(' ', "-")
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
        assert_eq!(cat.component_count(), 3);
        assert_eq!(cat.doc_count(), 1);
        assert!(cat.pages.len() >= 5);
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
}
