# Changelog

All notable changes to mcp-server-naiveui-docs.

## [Unreleased]

- Renamed crate, binary, cache dir, GitHub repo, and MCP server id from `mcp-server-naive-ui` / `naive-ui` to `mcp-server-naiveui-docs` / `naiveui-docs`. Indexed clone remains `{cache}/src/naive-ui`.

## [0.3.0] — 2026-09-16

Dogfood cut. `tools/list` is ten tools; version stays **0.3.0** unless that list changes.

### Added

- `naive_discrete` and `naive_theme` (PR5).
- README playbook, env table, MCP toml, security notes, skill-replacement operator checklist (PR6).
- `tests/test-stdio.sh` protocol smoke: initialize + `tools/list` (all ten names) + `naive_status` + `naive_search` + `naive_component` against `tests/fixtures/tree` (no network).

### Skill (outside this crate)

Operator checklist — rewrite `~/.grok/skills/naive-ui/SKILL.md` like the gpui skill:

1. Call the `naiveui-docs` MCP first.
2. Playbook identical to README / `get_info`.
3. Keep one jsDelivr fallback sentence at pin `v2.40.4`.
4. Keep StackChap IIFE wiring (`window.naive`, `n-config-provider`, `createDiscreteApi` once, `RemoteDataTable`, Glyphkit).

Do not vendor the skill file in this repo.

## [0.2.0]

Locator/reader tools: `naive_sync`, `naive_list`, `naive_search`, `naive_component`, `naive_prop`, `naive_demo`, `naive_get` (plus `naive_status`).

## [0.1.0]

Crate skeleton, stdio hello, ingest/sync, parser. `naive_status` only until 0.2.0.
