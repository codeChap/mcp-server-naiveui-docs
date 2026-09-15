#[path = "../src/names.rs"]
mod names;

use names::{
    Names, Resolve, kebab_from_heading, levenshtein, pascal_from_kebab, resolve, tag_from_kebab,
};

fn catalog() -> Vec<&'static str> {
    vec![
        "affix",
        "alert",
        "anchor",
        "auto-complete",
        "avatar",
        "button",
        "button-group",
        "config-provider",
        "data-table",
        "date-picker",
        "form",
        "h1",
        "layout",
        "layout-content",
        "layout-sider",
        "p",
        "text",
        "typography",
    ]
}

#[test]
fn kebab_tag_pascal_from_dir() {
    let n = Names::from_kebab("data-table");
    assert_eq!(n.kebab, "data-table");
    assert_eq!(n.tag, "n-data-table");
    assert_eq!(n.pascal, "NDataTable");
    assert_eq!(tag_from_kebab("auto-complete"), "n-auto-complete");
    assert_eq!(pascal_from_kebab("auto-complete"), "NAutoComplete");
    assert_eq!(pascal_from_kebab("config-provider"), "NConfigProvider");
    assert_eq!(pascal_from_kebab("h1"), "NH1");
}

#[test]
fn heading_layout_content_and_button_group() {
    let layout = Names::from_heading("Layout Content");
    assert_eq!(layout.kebab, "layout-content");
    assert_eq!(layout.tag, "n-layout-content");
    assert_eq!(layout.pascal, "NLayoutContent");
    assert_eq!(kebab_from_heading("ButtonGroup"), "button-group");
    assert_eq!(Names::from_heading("ButtonGroup").pascal, "NButtonGroup");
    assert_eq!(kebab_from_heading("H1"), "h1");
    assert_eq!(Names::from_heading("H1").pascal, "NH1");
}

#[test]
fn resolve_n_data_table_exact() {
    let ids = catalog();
    assert_eq!(
        resolve("n-data-table", &ids),
        Resolve::Hit("data-table".into())
    );
    assert_eq!(
        resolve("data-table", &ids),
        Resolve::Hit("data-table".into())
    );
}

#[test]
fn resolve_n_datatable_levenshtein() {
    let ids = catalog();
    assert_eq!(levenshtein("datatable", "data-table"), 1);
    assert_eq!(
        resolve("n-datatable", &ids),
        Resolve::Hit("data-table".into())
    );
}

#[test]
fn resolve_pascal_ndata_table() {
    let ids = catalog();
    assert_eq!(
        resolve("NDataTable", &ids),
        Resolve::Hit("data-table".into())
    );
    assert_eq!(
        resolve("DataTable", &ids),
        Resolve::Hit("data-table".into())
    );
}

#[test]
fn resolve_n_a_candidates_do_not_invent_a() {
    let ids = catalog();
    match resolve("n-a", &ids) {
        Resolve::Candidates(c) => {
            assert!(!c.is_empty(), "n-a should list prefix candidates");
            assert!(
                !c.iter().any(|id| id == "a"),
                "do not invent A as a typography alias, got {c:?}"
            );
            assert!(c.contains(&"alert".to_string()) || c.contains(&"avatar".to_string()));
        }
        other => panic!("expected candidates for n-a, got {other:?}"),
    }
}

#[test]
fn resolve_ambiguous_does_not_guess() {
    let ids = catalog();
    match resolve("a", &ids) {
        Resolve::Candidates(c) => {
            assert!(c.len() >= 2);
            assert!(!c.iter().any(|id| id == "a"));
        }
        other => panic!("expected candidates, got {other:?}"),
    }
}
