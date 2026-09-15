use std::path::PathBuf;

use rmcp::{
    ErrorData as McpError, ServerHandler, handler::server::tool::ToolRouter, model::*, tool,
    tool_handler, tool_router,
};

use crate::sources::{NAIVE_UI_PINNED_REV, REMOTE_ID, resolve_rev};

#[derive(Clone)]
pub struct NaiveUiServer {
    cache: PathBuf,
    tool_router: ToolRouter<Self>,
}

fn ok(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(msg.into())])
}

#[tool_router]
impl NaiveUiServer {
    pub fn new(cache: PathBuf) -> Self {
        Self {
            cache,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Catalog freshness: cache path, pin v2.40.4, page counts. Empty until sync. Call this first."
    )]
    async fn naive_status(&self) -> Result<CallToolResult, McpError> {
        let resolved = resolve_rev(std::env::var("NAIVE_UI_MCP_REV").ok().as_deref());
        let resolved_rev = resolved.as_deref().unwrap_or("HEAD");
        let body = serde_json::json!({
            "cache": self.cache.display().to_string(),
            "pin": NAIVE_UI_PINNED_REV,
            "resolved_rev": resolved_rev,
            "git_head": serde_json::Value::Null,
            "pin_match": false,
            "origin": serde_json::Value::Null,
            "pages": 0,
            "components": 0,
            "demos": 0,
            "docs": 0,
            "gotchas": 0,
            "missing": [REMOTE_ID],
            "schema_version": 1,
            "built_at": serde_json::Value::Null,
        });
        let text = serde_json::to_string_pretty(&body)
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
                 Call naive_status for cache path and pin (v2.40.4). \
                 Catalog is empty until sync (later).",
            )
    }
}
