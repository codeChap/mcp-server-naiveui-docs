# mcp-server-naive-ui

MCP server: Naive UI component docs, APIs, and demos from the pinned library source (`v2.40.4`).

Needs a Rust toolchain that supports edition 2024.

naiveui.com is a Vue SPA — do not `web_fetch` it; trust this server over training data.

## MCP config

```toml
[mcp_servers.naive-ui]
command = "/path/to/mcp-server-naive-ui"
enabled = true
startup_timeout_sec = 60
```

Call `naive_sync` once, then search/component before writing Vue.

## Cache / pin / env

- `NAIVE_UI_MCP_CACHE` — cache directory (never `/tmp`)
- `NAIVE_UI_MCP_REV` — override pin (default `v2.40.4`)
- `NAIVE_UI_MCP_SYNC_ON_START=1` — clone on boot
- CLI: `--sync` / `--sync --force` / `--rebuild`

## Playbook

1. Unknown component? `naive_search(query)` then `naive_component(name)`.
2. Known tag (`n-data-table`, `n-select`)? `naive_component` directly; ids accept `n-data-table`, `NDataTable`, `data-table`.
3. Looking for a prop across the lib (`remote`, `pagination`)? `naive_prop(name)`.
4. Need a usage snippet? `naive_component` demos list, then `naive_demo(component, name)`.
   On-disk files are `*.demo.vue` / `*.demo.md`, not the fence's `basic.vue`.
5. Prose guides and compiled pitfalls: `naive_get(id)` (`docs/customize-theme`, `gotchas`).
   Component ids: use `naive_component`, not `naive_get`.
6. Empty catalog / `pin_match` false: `naive_sync`.

Templates use kebab tags (`n-select`). `setup()` / `h()` uses Pascal (`NButton`) from `window.naive` in IIFE apps.

Tools in this version: `naive_status`, `naive_sync`, `naive_list`, `naive_search`, `naive_component`, `naive_prop`, `naive_demo`, `naive_get`.
