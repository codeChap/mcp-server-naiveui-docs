# mcp-server-naiveui-docs

[![CI](https://github.com/codeChap/mcp-server-naiveui-docs/actions/workflows/ci.yml/badge.svg)](https://github.com/codeChap/mcp-server-naiveui-docs/actions/workflows/ci.yml)

[naiveui.com](https://www.naiveui.com/en-US/os-theme/components) is a Vue SPA. `web_fetch` returns a shell with no prop tables. Training data invents APIs the pin never shipped. This stdio MCP clones [tusen-ai/naive-ui](https://github.com/tusen-ai/naive-ui) at `v2.40.4`, indexes the markdown and demos, and answers lookups offline. It is not a Naive UI runtime.

Needs rustc 1.85+ (edition 2024), plus `git`, `curl`, and `tar` on `PATH`. `HOME` must be set. Do not point `NAIVE_UI_MCP_CACHE` at `/tmp`, `/var/tmp`, `/dev/shm`, or a world-writable path.

## Install (for AI agents)

Four steps, in order. If you skip `--sync`, `naive_status` reports `components: 0`, `pin_match: false`, `missing: ["naive-ui"]`. Until the clone exists, only compiled gotchas are served.

### 1. Install the binary

```bash
cargo install --git https://github.com/codeChap/mcp-server-naiveui-docs
command -v mcp-server-naiveui-docs
```

That path is almost always `$HOME/.cargo/bin/mcp-server-naiveui-docs`. Copy it. MCP configs do not expand `$HOME`.

### 2. Clone and index the pin

```bash
mcp-server-naiveui-docs --sync
```

Network and disk writes. Checks out [tusen-ai/naive-ui](https://github.com/tusen-ai/naive-ui) at `v2.40.4` into `~/.cache/mcp-server-naiveui-docs/src/naive-ui`. Success at this pin prints `cloned at v2.40.4` then `indexed 116 pages, 735 demos`. Counts move if the pin changes.

Run this CLI sync before registering the server. `NAIVE_UI_MCP_SYNC_ON_START=1` clones during MCP launch and makes the first start slow.

### 3. Register stdio server `naiveui-docs`

Server id is `naiveui-docs`. `naive-ui` is the Vue library. Paste the path from step 1, not a placeholder. Restart the client after writing config so tools load.

Grok (`~/.grok/config.toml`):

```toml
[mcp_servers.naiveui-docs]
command = "/home/you/.cargo/bin/mcp-server-naiveui-docs"
args = []
enabled = true
startup_timeout_sec = 60
```

Claude / Cursor (`mcp.json` or Claude desktop config):

```json
{
  "mcpServers": {
    "naiveui-docs": {
      "command": "/home/you/.cargo/bin/mcp-server-naiveui-docs"
    }
  }
}
```

### 4. Confirm the catalog, then look things up

Call `naive_status`. Healthy means `pin_match` is true, `missing` is `[]`, and `components` is greater than 0. At this pin that is `components: 95`, `pages: 116`, `demos: 735`, `docs: 20`, `origin: "git"`. Extra keys (`cache`, `git_head`, `gotchas`, `schema_version`, `built_at`) are normal.

If the catalog is empty, call MCP `naive_sync`. `force=true` wipes the clone first. Then follow the playbook. Do not invent Naive props from training data. Do not `web_fetch` naiveui.com.

## Playbook

Same text as `ServerHandler::get_info` instructions:

1. Unknown component? `naive_search(query)` then `naive_component(name)`.
2. Known tag (`n-data-table`, `n-select`)? `naive_component` directly; ids accept `n-data-table`, `NDataTable`, `data-table`.
3. Looking for a prop across the lib (`remote`, `pagination`)? `naive_prop(name)`.
4. Need a usage snippet? `naive_component` demos list, then `naive_demo(component, name)`. On-disk files are `*.demo.vue` / `*.demo.md`, not the fence's `basic.vue`.
5. Toasts / confirms / loading bar outside setup: `naive_discrete` first. StackChap: `createDiscreteApi` once; `window.$message` / `$dialog` / `$notification`.
6. Theme tokens / `--n-*` CSS vars: `naive_theme(component?)`.
7. Prose guides and gotchas: `naive_get(id)` (`docs/customize-theme`, `gotchas`). Component ids: use `naive_component`, not `naive_get`.
8. Empty catalog / `pin_match` false: `naive_sync`.

Templates use kebab tags (`n-select`). `setup()` / `h()` uses Pascal (`NButton`) from `window.naive` in IIFE apps. Prefer Naive over homemade dropdowns, tables, dialogs.

Remote paging on a table, as a worked lookup:

1. `naive_prop` with `name=remote`
2. `naive_component` with `name=n-data-table` and `section=props`
3. `naive_demo` with `component=data-table` and `name=ajax-usage`

## Tools

Ten tools (`0.3.0`). Search, list, and prop are locators: ids and snippets. Component, demo, theme, discrete, and get are readers.

| Tool | Role | Use when |
|---|---|---|
| `naive_status` | freshness | First look. Empty catalog or `pin_match: false` → `naive_sync`. |
| `naive_sync` | networked ingest | Clone or refresh the pin. Network and disk writes. Optional `force=true` wipes the clone. |
| `naive_list` | locator | Browse rows (`category`, `kind`, `query`). Cap 200. Not API tables. |
| `naive_search` | locator | Unknown component. Hits: id + snippet ≤ 280 chars. Default limit 8, cap 20. |
| `naive_prop` | locator | Which components have this prop (`remote`, `pagination`, `row-key`). |
| `naive_component` | reader | Structured API for one page. Ids: `data-table`, `n-data-table`, `NDataTable`. Optional `section=`. |
| `naive_demo` | reader | One `*.demo.vue` / `*.demo.md` body, not the fence's `basic.vue`. Clip 24k. |
| `naive_theme` | reader | Common theme keys (literals only) and `--n-*` CSS vars. Optional `component=`. |
| `naive_discrete` | reader | `createDiscreteApi` (verbatim ts), caveats, related hooks. StackChap toasts. |
| `naive_get` | reader | Prose: `docs/customize-theme`, `gotchas`. Component ids → use `naive_component`. |

Gotchas are compiled into the binary (`include_str!`), so `cargo install` still serves them before the first clone.

## Cache, pin, env

Cache: `~/.cache/mcp-server-naiveui-docs`. Override with `NAIVE_UI_MCP_CACHE` or `XDG_CACHE_HOME`. On Windows, `LOCALAPPDATA` is the fallback. `HOME` or `NAIVE_UI_MCP_CACHE` is required.

`sync` checks out [tusen-ai/naive-ui](https://github.com/tusen-ai/naive-ui) at `v2.40.4`, the same rev as StackChap `naive-ui.iife.js`. Override with `NAIVE_UI_MCP_REV=<tag|sha>`, or `NAIVE_UI_MCP_REV=HEAD` to follow the default branch. See `naive_status`. Clone path: `{cache}/src/naive-ui`.

| Env | Default | Notes |
|---|---|---|
| `NAIVE_UI_MCP_CACHE` | `$XDG_CACHE_HOME/mcp-server-naiveui-docs` or `$HOME/.cache/mcp-server-naiveui-docs` | Never `/tmp`, `/var/tmp`, `/dev/shm`, or a world-writable path or parent, including when this env is set. |
| `NAIVE_UI_MCP_REV` | `v2.40.4` | Tag, SHA, or `HEAD` / `main` / `master` (unpinned). |
| `NAIVE_UI_MCP_SYNC_ON_START` | unset | `1` clones on launch. Slow first time. Prefer CLI `--sync` first. |
| `RUST_LOG` | unset | Tracing is stderr only. stdout is MCP JSON-RPC. |

CLI, not MCP: `--sync`, `--sync --force`, `--rebuild` (reparse markdown from the existing clone, no network). Stale sync lock: `rmdir ~/.cache/mcp-server-naiveui-docs/.sync.lock`.

## Build and test

For work on this crate. Install for use is above.

```bash
cargo build --release
./target/release/mcp-server-naiveui-docs --sync
cargo test
./tests/test-stdio.sh
```

Binary: `target/release/mcp-server-naiveui-docs`. Point the MCP `command` at that path while developing. Tests use `tests/fixtures/tree` as a fake cache and do not hit the network.

## Security

- No query-time HTTP. `naive_sync` and CLI `--sync` are the only networked paths. Git, GitHub archive, and jsDelivr hosts are allowlisted. There are no user-supplied URL params.
- Cache deny: after resolve, bail if the path is or is under `/tmp`, `/var/tmp`, or `/dev/shm`, or if the path or parent is world-writable (`mode & 0o002`). `NAIVE_UI_MCP_CACHE=/tmp` does not override this.
- Path traversal: `naive_demo` joins under `{clone}/src/{id}/demos/enUS/`, `canonicalize`s, and requires a prefix match. `..`, absolute names, and NUL are rejected.
- Stdio only. No bind, no listen, no API keys. `GIT_TERMINAL_PROMPT=0`.
- Tool JSON is never byte-sliced. String clips and omit-fields, with `truncated: true`. Search limit clamp 20.

## Skill replacement (operator)

The Grok skill lives outside this crate at `~/.grok/skills/naive-ui/SKILL.md`. Do not add it here.

Checklist, same shape as `~/.grok/skills/gpui/SKILL.md`:

1. Rewrite the skill to call the `naiveui-docs` MCP first. Do not invent props from training data or `web_fetch` naiveui.com.
2. Numbered playbook identical to `get_info` / this README.
3. Keep one jsDelivr fallback sentence (pin `v2.40.4`). It is not the primary lookup.
4. Keep StackChap product context the MCP does not replace: IIFE `window.naive`, kebab tags in templates, `n-config-provider` at the SPA root, `createDiscreteApi` once in `app.js`, `RemoteDataTable`, Glyphkit icons.

Rollback: remove the MCP config entry. The skill's jsDelivr sentence still works. Cache dir can be `rm -rf`'d; next `naive_sync` reclones.

## License

MIT. Indexed markdown and Vue come from [tusen-ai/naive-ui](https://github.com/tusen-ai/naive-ui) (MIT) at the pin.
