use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler, handler::server::tool::ToolRouter, model::*, tool,
    tool_handler, tool_router,
};

use crate::catalog::Catalog;
use crate::sources::{NAIVE_UI_PINNED_REV, clone_dir, resolve_rev};
use crate::sync::{git_rev, read_mcp_origin, same_git_rev};

#[derive(Clone)]
pub struct NaiveUiServer {
    cache: PathBuf,
    catalog: Arc<Catalog>,
    tool_router: ToolRouter<Self>,
}

fn ok(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(msg.into())])
}

#[tool_router]
impl NaiveUiServer {
    pub fn new(cache: PathBuf, catalog: Catalog) -> Self {
        Self {
            cache,
            catalog: Arc::new(catalog),
            tool_router: Self::tool_router(),
        }
    }

    fn status_json(&self) -> serde_json::Value {
        let resolved = resolve_rev(std::env::var("NAIVE_UI_MCP_REV").ok().as_deref());
        let resolved_rev = resolved.as_deref().unwrap_or("HEAD");
        let clone = clone_dir(&self.cache);
        let origin_pair = read_mcp_origin(&clone);
        let origin = origin_pair
            .as_ref()
            .map(|(o, _)| o.clone())
            .or_else(|| clone.join(".git").exists().then(|| "git".to_string()));
        let recorded = origin_pair.as_ref().map(|(_, r)| r.as_str());
        let git_head = if origin.as_deref() == Some("git") {
            git_rev(&clone).ok()
        } else {
            None
        };
        let pin_match = match (recorded, git_head.as_deref()) {
            (Some(rec), _) if rec == resolved_rev || same_git_rev(rec, resolved_rev) => true,
            (_, Some(head)) if same_git_rev(head, resolved_rev) => true,
            _ => false,
        };
        let cat = self.catalog.as_ref();
        serde_json::json!({
            "cache": self.cache.display().to_string(),
            "pin": NAIVE_UI_PINNED_REV,
            "resolved_rev": resolved_rev,
            "git_head": git_head,
            "pin_match": pin_match,
            "origin": origin,
            "pages": cat.pages.len(),
            "components": cat.component_count(),
            "demos": cat.demo_count(),
            "docs": cat.doc_count(),
            "gotchas": cat.gotchas_count(),
            "missing": cat.missing,
            "schema_version": 1,
            "built_at": format!("unix:{}", cat.built_at),
        })
    }

    #[tool(
        description = "Catalog freshness: cache path, pin, page/demo counts, origin. Empty until sync. Call this first."
    )]
    async fn naive_status(&self) -> Result<CallToolResult, McpError> {
        let text = serde_json::to_string_pretty(&self.status_json())
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(ok(text))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for NaiveUiServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("naive-ui", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Naive UI docs server. naiveui.com is a Vue SPA — do not web_fetch it. \
                 Call naive_status for cache path, pin (v2.40.4), and catalog counts. \
                 Empty catalog / missing naive-ui: run the binary with --sync.",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    fn fixture_cache() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tree")
    }

    #[test]
    fn status_from_fixture_tree() {
        let cache = fixture_cache();
        let cat = Catalog::load(&cache);
        let srv = NaiveUiServer::new(cache.clone(), cat);
        let v = srv.status_json();
        assert_eq!(v["gotchas"], 1);
        assert!(v["pages"].as_u64().unwrap() >= 5);
        assert_eq!(v["components"], 3);
        assert_eq!(v["docs"], 1);
        assert!(v["missing"].as_array().unwrap().is_empty());
        assert_eq!(v["origin"], "archive");
        assert_eq!(v["pin_match"], serde_json::json!(true));
        assert_eq!(v["pin"], "v2.40.4");
        assert!(v["built_at"].as_str().unwrap().starts_with("unix:"));
        assert_eq!(v["cache"], cache.display().to_string());
    }
}
