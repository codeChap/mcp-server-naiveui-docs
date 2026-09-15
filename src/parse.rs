//! Line-oriented walker: `index.demo-entry.md` → structured [`Page`].
//! No CommonMark crate — Naive's files are a regular ATX + GFM-table subset.
#![allow(dead_code)]

use serde::Serialize;

use crate::names::Names;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PageKind {
    Component,
    Api,
    Config,
    Doc,
    Gotchas,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiKind {
    Props,
    Slots,
    Events,
    Methods,
    Properties,
    Type,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: PageKind,
    pub tags: Vec<String>,
    pub pascals: Vec<String>,
    pub components: Vec<String>,
    pub category: String,
    pub site_url: String,
    pub source_path: String,
    pub version_hint: Option<String>,
    pub demos: Vec<DemoRef>,
    pub apis: Vec<ApiSection>,
    pub extra_types: Vec<TypeBlock>,
    pub alerts: Vec<String>,
    pub qa_markdown: Option<String>,
    pub extra_sections: Vec<NamedMarkdown>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DemoRef {
    pub fence_id: String,
    pub file_name: String,
    pub debug: bool,
    pub title: Option<String>,
}

impl DemoRef {
    /// Naive `resolveDemoInfos`: `.vue` in the fence id → `{stem}.demo.vue`, else `{id}.demo.md`.
    pub fn from_fence_id(fence_id: &str) -> Self {
        let debug = fence_id.contains("debug") || fence_id.contains("Debug");
        let file_name = if fence_id.contains(".vue") {
            let stem_end = fence_id.len().saturating_sub(4);
            format!("{}.demo.vue", &fence_id[..stem_end])
        } else {
            format!("{fence_id}.demo.md")
        };
        Self {
            fence_id: fence_id.to_string(),
            file_name,
            debug,
            title: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiSection {
    pub heading: String,
    pub owners: Vec<String>,
    pub kind: ApiKind,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeBlock {
    pub heading: String,
    pub language: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NamedMarkdown {
    pub heading: String,
    pub markdown: String,
}

pub fn page_kind(id: &str, source_path: &str) -> PageKind {
    if id == "gotchas" {
        return PageKind::Gotchas;
    }
    if source_path.contains("demo/pages/docs/") {
        return PageKind::Doc;
    }
    if id == "discrete" {
        return PageKind::Api;
    }
    if matches!(id, "config-provider" | "config-consumer" | "global-style") {
        return PageKind::Config;
    }
    PageKind::Component
}

pub fn parse_page(id: &str, markdown: &str, source_path: &str) -> Page {
    Parser::new(id, markdown, source_path).run()
}

/// First ATX H1 inside `<markdown>…</markdown>`, else the first H1 in the file.
pub fn extract_demo_title(source: &str) -> Option<String> {
    if let Some(start) = source.find("<markdown>")
        && let Some(rel_end) = source[start..].find("</markdown>")
        && let Some(title) = first_h1(&source[start..start + rel_end])
    {
        return Some(title);
    }
    first_h1(source)
}

fn first_h1(s: &str) -> Option<String> {
    for line in s.lines() {
        if let Some((1, title)) = parse_atx(line)
            && !title.is_empty()
        {
            return Some(title.to_string());
        }
    }
    None
}

enum Section {
    Start,
    Description,
    Demos,
    ApiContainer,
    Api(ApiDraft),
    CreateDiscrete { heading: String },
    Qa,
    Extra { heading: String },
}

struct ApiDraft {
    heading: String,
    kind: ApiKind,
    owners: Vec<String>,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}

struct Parser<'a> {
    id: &'a str,
    source_path: &'a str,
    lines: Vec<&'a str>,
    kind: PageKind,
    title: String,
    version_hint: Option<String>,
    description: String,
    demos: Vec<DemoRef>,
    apis: Vec<ApiSection>,
    extra_types: Vec<TypeBlock>,
    alerts: Vec<String>,
    qa: String,
    extra_sections: Vec<NamedMarkdown>,
    extra_buf: String,
    section: Section,
    components: Vec<String>,
}

impl<'a> Parser<'a> {
    fn new(id: &'a str, markdown: &'a str, source_path: &'a str) -> Self {
        Self {
            id,
            source_path,
            lines: markdown.lines().collect(),
            kind: page_kind(id, source_path),
            title: String::new(),
            version_hint: None,
            description: String::new(),
            demos: Vec::new(),
            apis: Vec::new(),
            extra_types: Vec::new(),
            alerts: Vec::new(),
            qa: String::new(),
            extra_sections: Vec::new(),
            extra_buf: String::new(),
            section: Section::Start,
            components: Vec::new(),
        }
    }

    fn run(mut self) -> Page {
        let mut i = 0;
        while i < self.lines.len() {
            let line = self.lines[i];

            if let Some((depth, heading)) = parse_atx(line) {
                self.handle_heading(depth, heading, line);
                i += 1;
                continue;
            }

            if let Some((lang, body, next)) = take_fence(&self.lines, i) {
                self.handle_fence(i, next, &lang, &body);
                i = next;
                continue;
            }

            if is_table_row(line) {
                let next = self.handle_table(i);
                i = next;
                continue;
            }

            if let Some((text, next)) = take_alert(&self.lines, i) {
                self.alerts.push(text);
                let cap = capture(&self.section);
                if matches!(cap, Capture::Qa | Capture::Extra) {
                    let chunk = self.lines[i..next].join("\n");
                    match cap {
                        Capture::Qa => append_chunk(&mut self.qa, &chunk),
                        Capture::Extra => append_chunk(&mut self.extra_buf, &chunk),
                        _ => {}
                    }
                }
                i = next;
                continue;
            }

            if let Some(next) = take_comment(&self.lines, i) {
                i = next;
                continue;
            }

            match capture(&self.section) {
                Capture::Description => append_line(&mut self.description, line),
                Capture::Qa => append_line(&mut self.qa, line),
                Capture::Extra => append_line(&mut self.extra_buf, line),
                // Skip HTML/prose until a table, fence, or next heading (DataTable Methods, FormItemRule).
                Capture::None => {}
            }
            i += 1;
        }
        self.finalize();
        self.into_page()
    }

    fn handle_heading(&mut self, depth: u8, heading: &str, raw_line: &str) {
        if depth == 1 {
            if self.title.is_empty() {
                let (title, hint) = parse_h1_title(heading);
                self.title = title;
                self.version_hint = hint;
                self.section = Section::Description;
            }
            return;
        }
        if depth >= 3 {
            match capture(&self.section) {
                Capture::Qa => {
                    append_line(&mut self.qa, raw_line);
                    return;
                }
                Capture::Extra => {
                    append_line(&mut self.extra_buf, raw_line);
                    return;
                }
                _ => {}
            }
        }
        self.finalize();
        if depth == 2 {
            self.section = start_h2(heading);
            let owners = match &self.section {
                Section::Api(draft) => draft.owners.clone(),
                _ => Vec::new(),
            };
            push_unique(&mut self.components, &owners);
            return;
        }
        // H3–H4: API tables are not parented on ## API (config-consumer Events/Slots).
        if heading == "createDiscreteApi" || heading.starts_with("createDiscreteApi ") {
            self.section = Section::CreateDiscrete {
                heading: heading.to_string(),
            };
            return;
        }
        let (kind, owners) = classify_api_heading(heading);
        push_unique(&mut self.components, &owners);
        self.section = Section::Api(ApiDraft {
            heading: heading.to_string(),
            kind,
            owners,
            columns: Vec::new(),
            rows: Vec::new(),
        });
    }

    fn handle_fence(&mut self, from: usize, to: usize, lang: &str, body: &str) {
        if is_demo_lang(lang) {
            for id in body.lines().map(str::trim).filter(|s| !s.is_empty()) {
                self.demos.push(DemoRef::from_fence_id(id));
            }
            return;
        }
        let api_heading = match &self.section {
            Section::Api(draft) => Some(draft.heading.clone()),
            Section::CreateDiscrete { heading } => Some(heading.clone()),
            _ => None,
        };
        if is_ts_js(lang)
            && let Some(heading) = api_heading
        {
            self.extra_types.push(TypeBlock {
                heading,
                language: lang.to_string(),
                body: body.to_string(),
            });
            return;
        }
        match capture(&self.section) {
            Capture::Description => append_range(&mut self.description, &self.lines, from, to),
            Capture::Qa => append_range(&mut self.qa, &self.lines, from, to),
            Capture::Extra => append_range(&mut self.extra_buf, &self.lines, from, to),
            Capture::None => {}
        }
    }

    fn handle_table(&mut self, i: usize) -> usize {
        let Some((parsed, next)) = take_table(&self.lines, i) else {
            return i + 1;
        };
        if matches!(self.section, Section::Api(_)) {
            if let Section::Api(draft) = &mut self.section {
                apply_table(draft, parsed);
            }
            return next;
        }
        match capture(&self.section) {
            Capture::Qa => append_range(&mut self.qa, &self.lines, i, next),
            Capture::Extra => append_range(&mut self.extra_buf, &self.lines, i, next),
            Capture::Description => append_range(&mut self.description, &self.lines, i, next),
            Capture::None => {}
        }
        next
    }

    fn finalize(&mut self) {
        match std::mem::replace(&mut self.section, Section::Start) {
            Section::Api(draft) => {
                if draft.kind == ApiKind::Other && draft.columns.is_empty() && draft.rows.is_empty()
                {
                    // Grouping headings such as "MessageProvider Injection API".
                } else {
                    self.apis.push(ApiSection {
                        heading: draft.heading,
                        owners: draft.owners,
                        kind: draft.kind,
                        columns: draft.columns,
                        rows: draft.rows,
                    });
                }
            }
            Section::Extra { heading } => {
                let md = trim_md(&self.extra_buf);
                self.extra_buf.clear();
                if !md.is_empty() {
                    self.extra_sections.push(NamedMarkdown {
                        heading,
                        markdown: md,
                    });
                }
            }
            Section::Qa => {}
            _ => {}
        }
        self.section = Section::Start;
    }

    fn into_page(self) -> Page {
        let names = Names::from_kebab(self.id);
        let site_url = site_url(self.id, self.kind, self.source_path);
        let qa = trim_md(&self.qa);
        Page {
            id: self.id.to_string(),
            title: self.title,
            description: trim_md(&self.description),
            kind: self.kind,
            tags: vec![names.tag],
            pascals: vec![names.pascal],
            components: self.components,
            category: String::new(),
            site_url,
            source_path: self.source_path.to_string(),
            version_hint: self.version_hint,
            demos: self.demos,
            apis: self.apis,
            extra_types: self.extra_types,
            alerts: self.alerts,
            qa_markdown: if qa.is_empty() { None } else { Some(qa) },
            extra_sections: self.extra_sections,
        }
    }
}

fn start_h2(heading: &str) -> Section {
    if is_qa(heading) {
        return Section::Qa;
    }
    if heading.eq_ignore_ascii_case("demos") || heading.eq_ignore_ascii_case("demo") {
        return Section::Demos;
    }
    if heading.eq_ignore_ascii_case("api") {
        return Section::ApiContainer;
    }
    if let Some(kind) = api_kind_suffix(heading) {
        let owners = owners_from_heading(heading, kind);
        return Section::Api(ApiDraft {
            heading: heading.to_string(),
            kind,
            owners,
            columns: Vec::new(),
            rows: Vec::new(),
        });
    }
    Section::Extra {
        heading: heading.to_string(),
    }
}

fn classify_api_heading(heading: &str) -> (ApiKind, Vec<String>) {
    if let Some(kind) = api_kind_suffix(heading) {
        (kind, owners_from_heading(heading, kind))
    } else {
        (ApiKind::Other, Vec::new())
    }
}

fn api_kind_suffix(heading: &str) -> Option<ApiKind> {
    let h = heading.trim();
    if h == "Properties" || h.ends_with(" Properties") {
        Some(ApiKind::Properties)
    } else if h == "Props" || h.ends_with(" Props") {
        Some(ApiKind::Props)
    } else if h == "Slots" || h.ends_with(" Slots") {
        Some(ApiKind::Slots)
    } else if h == "Events" || h.ends_with(" Events") {
        Some(ApiKind::Events)
    } else if h == "Methods" || h.ends_with(" Methods") {
        Some(ApiKind::Methods)
    } else if h == "Type" || h.ends_with(" Type") {
        Some(ApiKind::Type)
    } else {
        None
    }
}

fn owners_from_heading(heading: &str, kind: ApiKind) -> Vec<String> {
    let suffix = match kind {
        ApiKind::Properties => "Properties",
        ApiKind::Props => "Props",
        ApiKind::Slots => "Slots",
        ApiKind::Events => "Events",
        ApiKind::Methods => "Methods",
        ApiKind::Type => "Type",
        ApiKind::Other => return Vec::new(),
    };
    let rest = heading
        .trim()
        .strip_suffix(suffix)
        .unwrap_or(heading)
        .trim();
    rest.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn apply_table(draft: &mut ApiDraft, parsed: Vec<Vec<String>>) {
    if parsed.is_empty() {
        return;
    }
    if !draft.columns.is_empty() {
        tracing::warn!(
            heading = draft.heading.as_str(),
            "extra table under API heading ignored"
        );
        return;
    }
    draft.columns = parsed[0].clone();
    let n = draft.columns.len();
    draft.rows = parsed
        .into_iter()
        .skip(1)
        .map(|row| pad_row(row, n))
        .collect();
}

fn pad_row(mut row: Vec<String>, n: usize) -> Vec<String> {
    while row.len() < n {
        row.push(String::new());
    }
    row
}

fn site_url(id: &str, kind: PageKind, source_path: &str) -> String {
    match kind {
        PageKind::Doc => {
            let slug = docs_slug(source_path).unwrap_or_else(|| id.to_string());
            format!("https://www.naiveui.com/en-US/os-theme/docs/{slug}")
        }
        PageKind::Gotchas => String::new(),
        _ => format!("https://www.naiveui.com/en-US/os-theme/components/{id}"),
    }
}

fn docs_slug(source_path: &str) -> Option<String> {
    const MARKER: &str = "demo/pages/docs/";
    let rest = source_path.split(MARKER).nth(1)?;
    let slug = rest.split('/').next()?;
    if slug.is_empty() {
        None
    } else {
        Some(slug.to_string())
    }
}

#[derive(Clone, Copy)]
enum Capture {
    Description,
    Qa,
    Extra,
    None,
}

fn capture(section: &Section) -> Capture {
    match section {
        Section::Description => Capture::Description,
        Section::Qa => Capture::Qa,
        Section::Extra { .. } => Capture::Extra,
        _ => Capture::None,
    }
}

fn is_qa(heading: &str) -> bool {
    let h = heading.trim();
    h.eq_ignore_ascii_case("q & a")
        || h.eq_ignore_ascii_case("q&a")
        || h.eq_ignore_ascii_case("faq")
        || h.eq_ignore_ascii_case("q and a")
}

fn is_demo_lang(lang: &str) -> bool {
    lang.trim().eq_ignore_ascii_case("demo")
}

fn is_ts_js(lang: &str) -> bool {
    matches!(
        lang.trim().to_ascii_lowercase().as_str(),
        "ts" | "js" | "typescript" | "javascript"
    )
}

fn parse_h1_title(heading: &str) -> (String, Option<String>) {
    let heading = heading.trim();
    // Naive mixes full-width `（` with ASCII `)` on Discrete API（v2.29.0)
    let open = heading
        .rfind('（')
        .into_iter()
        .chain(heading.rfind('('))
        .max();
    if let Some(idx) = open {
        let rest = &heading[idx..];
        let inner = rest
            .trim_start_matches('（')
            .trim_start_matches('(')
            .trim_end_matches('）')
            .trim_end_matches(')')
            .trim();
        if rest.ends_with('）') || rest.ends_with(')') {
            let hint = inner.strip_prefix('v').unwrap_or(inner);
            let main = heading[..idx].trim();
            if !main.is_empty() && !hint.is_empty() {
                return (main.to_string(), Some(hint.to_string()));
            }
        }
    }
    (heading.to_string(), None)
}

fn parse_atx(line: &str) -> Option<(u8, &str)> {
    let mut i = 0;
    let bytes = line.as_bytes();
    while i < bytes.len() && i < 3 && bytes[i] == b' ' {
        i += 1;
    }
    let rest = line.get(i..)?;
    let depth = rest.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&depth) {
        return None;
    }
    let after = rest.get(depth..)?;
    if after.is_empty() {
        return Some((depth as u8, ""));
    }
    if !after.starts_with(' ') && !after.starts_with('\t') {
        return None;
    }
    Some((depth as u8, after.trim()))
}

fn take_fence(lines: &[&str], i: usize) -> Option<(String, String, usize)> {
    let trimmed = lines[i].trim_start();
    let rest = trimmed.strip_prefix("```")?;
    let lang = rest.trim().to_string();
    let mut body = Vec::new();
    let mut j = i + 1;
    while j < lines.len() {
        if lines[j].trim_start().starts_with("```") {
            return Some((lang, body.join("\n"), j + 1));
        }
        body.push(lines[j]);
        j += 1;
    }
    tracing::warn!("unclosed fence starting at line {}", i + 1);
    Some((lang, body.join("\n"), lines.len()))
}

fn take_table(lines: &[&str], i: usize) -> Option<(Vec<Vec<String>>, usize)> {
    if !is_table_row(lines[i]) {
        return None;
    }
    let mut raw = Vec::new();
    let mut j = i;
    while j < lines.len() && is_table_row(lines[j]) {
        raw.push(split_table_row(lines[j]));
        j += 1;
    }
    if raw.is_empty() {
        return None;
    }
    let mut parsed = Vec::new();
    for (idx, row) in raw.into_iter().enumerate() {
        if idx > 0 && !row.is_empty() && row.iter().all(|c| is_alignment_cell(c)) {
            continue;
        }
        parsed.push(row);
    }
    Some((parsed, j))
}

fn is_table_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// Split on `|` whose preceding char is not `\`, then unescape `\|`. Do not "fix" quotes.
fn split_table_row(line: &str) -> Vec<String> {
    let mut line = line.trim();
    if let Some(rest) = line.strip_prefix('|') {
        line = rest;
    }
    if line.ends_with('|') && !line.ends_with("\\|") {
        line = &line[..line.len() - 1];
    }
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut prev_backslash = false;
    for c in line.chars() {
        if c == '|' && !prev_backslash {
            cells.push(unescape_cell(&cur));
            cur.clear();
            prev_backslash = false;
            continue;
        }
        cur.push(c);
        prev_backslash = c == '\\';
    }
    cells.push(unescape_cell(&cur));
    cells
}

fn unescape_cell(s: &str) -> String {
    s.trim().replace("\\|", "|")
}

fn is_alignment_cell(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let t = s.trim_matches(':');
    !t.is_empty() && t.chars().all(|c| c == '-')
}

fn take_comment(lines: &[&str], i: usize) -> Option<usize> {
    let t = lines[i].trim();
    if !t.contains("<!--") {
        return None;
    }
    if !t.starts_with("<!--") {
        return None;
    }
    if t.contains("-->") {
        return Some(i + 1);
    }
    let mut j = i + 1;
    while j < lines.len() {
        if lines[j].contains("-->") {
            return Some(j + 1);
        }
        j += 1;
    }
    Some(lines.len())
}

fn take_alert(lines: &[&str], i: usize) -> Option<(String, usize)> {
    let lower = lines[i].to_ascii_lowercase();
    let rel = lower.find("<n-alert")?;
    let open_gt = lines[i][rel..].find('>')?;
    let after_open = rel + open_gt + 1;
    let after = &lines[i][after_open..];
    if let Some(end) = find_ci(after, "</n-alert>") {
        return Some((strip_tags(&after[..end]), i + 1));
    }
    let mut buf = after.to_string();
    let mut j = i + 1;
    while j < lines.len() {
        if let Some(end) = find_ci(lines[j], "</n-alert>") {
            buf.push('\n');
            buf.push_str(&lines[j][..end]);
            return Some((strip_tags(&buf), j + 1));
        }
        buf.push('\n');
        buf.push_str(lines[j]);
        j += 1;
    }
    Some((strip_tags(&buf), lines.len()))
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase().find(&needle.to_ascii_lowercase())
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn append_line(buf: &mut String, line: &str) {
    if !buf.is_empty() {
        buf.push('\n');
    }
    buf.push_str(line);
}

fn append_chunk(buf: &mut String, chunk: &str) {
    if chunk.is_empty() {
        return;
    }
    if !buf.is_empty() {
        buf.push('\n');
    }
    buf.push_str(chunk);
}

fn append_range(buf: &mut String, lines: &[&str], from: usize, to: usize) {
    for line in &lines[from..to] {
        append_line(buf, line);
    }
}

fn trim_md(s: &str) -> String {
    s.trim().to_string()
}

fn push_unique(vec: &mut Vec<String>, items: &[String]) {
    for item in items {
        if !vec.iter().any(|x| x == item) {
            vec.push(item.clone());
        }
    }
}
