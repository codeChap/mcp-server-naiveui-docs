#[path = "../src/names.rs"]
mod names;
#[path = "../src/parse.rs"]
mod parse;

use std::path::PathBuf;

use parse::{ApiKind, DemoRef, Page, PageKind, extract_demo_title, page_kind, parse_page};

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn parse_id(id: &str) -> Page {
    let md = fixture(&format!("{id}.demo-entry.md"));
    parse_page(id, &md, &format!("src/{id}/demos/enUS/index.demo-entry.md"))
}

fn section<'a>(page: &'a Page, contains: &str) -> &'a parse::ApiSection {
    page.apis
        .iter()
        .find(|s| s.heading.contains(contains))
        .unwrap_or_else(|| {
            panic!(
                "missing section {contains} in {:?}",
                page.apis.iter().map(|s| &s.heading).collect::<Vec<_>>()
            )
        })
}

fn row_named<'a>(sec: &'a parse::ApiSection, name: &str) -> &'a [String] {
    sec.rows
        .iter()
        .find(|r| r.first().map(|c| c.as_str()) == Some(name))
        .map(|r| r.as_slice())
        .unwrap_or_else(|| panic!("missing row {name} in {}", sec.heading))
}

#[test]
fn button_attr_type() {
    let page = parse_id("button");
    assert_eq!(page.title, "Button");
    assert_eq!(page.kind, PageKind::Component);
    let props = section(&page, "Button Props");
    let row = row_named(props, "attr-type");
    assert!(
        row[1].contains("'button' | 'submit' | 'reset'"),
        "unescaped pipes: {}",
        row[1]
    );
    assert!(!row[1].contains("\\|"), "escaped pipes should be unescaped");
}

#[test]
fn data_table_remote_and_download_csv() {
    let page = parse_id("data-table");
    let props = section(&page, "DataTable Props");
    let remote = row_named(props, "remote");
    assert_eq!(remote[0], "remote");
    assert!(remote.iter().any(|c| c.contains("boolean")));

    let methods = section(&page, "DataTable Methods");
    assert_eq!(methods.kind, ApiKind::Methods);
    let csv = row_named(methods, "downloadCsv");
    assert!(
        csv.iter()
            .any(|c| c.contains("Download CSV") || c.contains("fileName"))
    );
}

#[test]
fn layout_has_sider_and_sider_owner() {
    let page = parse_id("layout");
    let props = section(&page, "Layout, Layout Content Props");
    assert!(props.owners.contains(&"Layout".to_string()));
    assert!(props.owners.contains(&"Layout Content".to_string()));
    let _ = row_named(props, "has-sider");

    let sider = section(&page, "Layout Sider Props");
    assert!(
        sider.owners.iter().any(|o| o == "Layout Sider"),
        "owners={:?}",
        sider.owners
    );
    assert!(page.components.iter().any(|c| c == "Layout Sider"));
    assert!(
        page.extra_sections
            .iter()
            .any(|s| s.heading.contains("Changes After v2.3.0"))
    );
}

#[test]
fn form_item_rule_level_is_type_table_not_extra_types() {
    let page = parse_id("form");
    let rule = page
        .apis
        .iter()
        .find(|s| s.heading.contains("FormItemRule") && s.kind == ApiKind::Type)
        .expect("FormItemRule Type ApiSection");
    let level = row_named(rule, "level");
    assert!(
        level
            .iter()
            .any(|c| c.contains("'error'") && c.contains("'warning'")),
        "level row: {level:?}"
    );
    assert!(
        !page
            .extra_types
            .iter()
            .any(|t| t.heading.contains("FormItemRule")),
        "FormItemRule is a table in apis, not extra_types"
    );

    let empty_type = page
        .apis
        .iter()
        .find(|s| s.heading.contains("FormValidateMessages") && s.kind == ApiKind::Type)
        .expect("empty FormValidateMessages Type");
    assert!(empty_type.rows.is_empty());

    let gi = section(&page, "FormItemGi Props");
    assert!(gi.rows.is_empty(), "prose-only FormItemGi Props");
}

#[test]
fn discrete_create_api_verbatim_no_modal_in_includes() {
    let page = parse_id("discrete");
    assert_eq!(page.title, "Discrete API");
    assert_eq!(page.kind, PageKind::Api);
    assert_eq!(page.version_hint.as_deref(), Some("2.29.0"));
    let block = page
        .extra_types
        .iter()
        .find(|t| t.heading.contains("createDiscreteApi"))
        .expect("createDiscreteApi TypeBlock");
    assert_eq!(block.language, "ts");
    assert!(
        block
            .body
            .contains("Array<'message' | 'dialog' | 'notification' | 'loadingBar'>"),
        "verbatim includes union: {}",
        block.body
    );
    assert!(
        !block.body.contains("'modal'"),
        "v2.40.4 includes union must not contain 'modal'"
    );
    assert!(block.body.contains("modal: ModalApi"));
}

#[test]
fn config_consumer_events_without_api_heading() {
    let page = parse_id("config-consumer");
    assert_eq!(page.kind, PageKind::Config);
    assert!(
        page.apis.iter().any(|s| s.kind == ApiKind::Events),
        "Events despite no ## API: {:?}",
        page.apis.iter().map(|s| &s.heading).collect::<Vec<_>>()
    );
    assert!(page.apis.iter().any(|s| s.kind == ApiKind::Slots));
    let basic = page
        .demos
        .iter()
        .find(|d| d.fence_id == "basic")
        .expect("basic fence");
    assert_eq!(basic.file_name, "basic.demo.md");
    assert!(!basic.debug);
}

#[test]
fn ajax_usage_maps_to_demo_md() {
    let page = parse_id("data-table");
    let ajax = page
        .demos
        .iter()
        .find(|d| d.fence_id == "ajax-usage")
        .expect("ajax-usage fence");
    assert_eq!(ajax.file_name, "ajax-usage.demo.md");
    let basic = page
        .demos
        .iter()
        .find(|d| d.fence_id == "basic.vue")
        .expect("basic.vue fence");
    assert_eq!(basic.file_name, "basic.demo.vue");
}

#[test]
fn avatar_v_show_debug_is_debug() {
    let page = parse_id("avatar");
    let debug = page
        .demos
        .iter()
        .find(|d| d.fence_id.contains("debug") || d.fence_id.contains("Debug"))
        .expect("debug demo in avatar fixture");
    assert!(debug.debug);
    assert_eq!(debug.fence_id, "v-show-debug.vue");
    assert_eq!(debug.file_name, "v-show-debug.demo.vue");
}

#[test]
fn synthetic_debug_fence() {
    let md = "# X\n\n```demo\nv-show-debug.vue\nbasic.vue\n```\n";
    let page = parse_page("x", md, "src/x/demos/enUS/index.demo-entry.md");
    let debug = page
        .demos
        .iter()
        .find(|d| d.fence_id == "v-show-debug.vue")
        .unwrap();
    assert!(debug.debug);
    let basic = page
        .demos
        .iter()
        .find(|d| d.fence_id == "basic.vue")
        .unwrap();
    assert!(!basic.debug);
}

#[test]
fn typography_owners_include_text_p_h1_not_a() {
    let page = parse_id("typography");
    assert!(
        page.components.iter().any(|c| c == "Text"),
        "{:?}",
        page.components
    );
    assert!(
        page.components.iter().any(|c| c == "P"),
        "{:?}",
        page.components
    );
    assert!(
        page.components.iter().any(|c| c == "H1"),
        "{:?}",
        page.components
    );
    assert!(
        !page.components.iter().any(|c| c == "A"),
        "no A owner: {:?}",
        page.components
    );
}

#[test]
fn global_style_empty_apis() {
    let page = parse_id("global-style");
    assert_eq!(page.kind, PageKind::Config);
    assert!(page.apis.is_empty());
    assert!(page.extra_sections.iter().any(|s| s.heading == "Usage"));
}

#[test]
fn demo_title_extraction() {
    let vue = fixture("basic.demo.vue");
    assert_eq!(extract_demo_title(&vue).as_deref(), Some("Basic"));
    let md = fixture("ajax-usage.demo.md");
    assert_eq!(extract_demo_title(&md).as_deref(), Some("Async"));
}

#[test]
fn demo_ref_mapping() {
    let vue = DemoRef::from_fence_id("basic.vue");
    assert_eq!(vue.file_name, "basic.demo.vue");
    let md = DemoRef::from_fence_id("ajax-usage");
    assert_eq!(md.file_name, "ajax-usage.demo.md");
    let debug = DemoRef::from_fence_id("v-show-debug.vue");
    assert!(debug.debug);
}

#[test]
fn page_kind_matrix() {
    assert_eq!(page_kind("gotchas", "data/gotchas.md"), PageKind::Gotchas);
    assert_eq!(
        page_kind(
            "customize-theme",
            "demo/pages/docs/customize-theme/enUS/index.md"
        ),
        PageKind::Doc
    );
    assert_eq!(
        page_kind("discrete", "src/discrete/demos/enUS/index.demo-entry.md"),
        PageKind::Api
    );
    assert_eq!(
        page_kind(
            "config-consumer",
            "src/config-consumer/demos/enUS/index.demo-entry.md"
        ),
        PageKind::Config
    );
    assert_eq!(
        page_kind("button", "src/button/demos/enUS/index.demo-entry.md"),
        PageKind::Component
    );
}

#[test]
fn message_qa_and_alerts() {
    let page = parse_id("message");
    assert!(page.qa_markdown.is_some());
    assert!(
        page.qa_markdown
            .as_ref()
            .unwrap()
            .contains("createDiscreteApi")
    );
    assert!(!page.alerts.is_empty());
    assert!(
        page.extra_types
            .iter()
            .any(|t| t.heading.contains("MessageRenderMessage"))
    );
}

#[test]
fn empty_markdown_does_not_panic() {
    let page = parse_page("empty", "", "src/empty/demos/enUS/index.demo-entry.md");
    assert!(page.apis.is_empty());
    assert!(page.demos.is_empty());
    assert_eq!(page.title, "");
}

#[test]
fn leading_html_comment_skipped_before_h1() {
    let md = "<!-- fixture from tusen-ai/naive-ui v2.40.4 -->\n# Button\n\nHello.\n";
    let page = parse_page("button", md, "src/button/demos/enUS/index.demo-entry.md");
    assert_eq!(page.title, "Button");
    assert_eq!(page.description, "Hello.");
}

#[test]
fn data_table_column_fixed_quotes_not_rewritten() {
    let page = parse_id("data-table");
    let cols = section(&page, "DataTableColumn Properties");
    let fixed = row_named(cols, "fixed");
    assert!(
        fixed[1].contains("'left | 'right' | false") || fixed[1].contains("'left"),
        "pass through broken quotes: {}",
        fixed[1]
    );
}
