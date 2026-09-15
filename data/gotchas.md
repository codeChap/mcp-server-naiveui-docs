# Gotchas

Compiled pitfalls for Naive UI at the pin. Always present even before the first clone.

## Do not fetch naiveui.com

naiveui.com is a Vue SPA. `web_fetch` returns a shell with no prop tables. The source of truth is the library repo (this server clones it). Citations may use the website URL; never scrape the site.

## IIFE vs ESM

StackChap-family SPAs load `public/assets/admin/vendor/naive-ui.iife.js` → `window.naive`.
ESM apps `import { NButton, createDiscreteApi } from 'naive-ui'`.
APIs in the markdown apply to both. **New** props added after 2.40.4 do not exist on the IIFE until the vendor file is bumped. `naive_status.pin` is the contract.

## createDiscreteApi once

Call `naive.createDiscreteApi` **once** in `app.js`. Assign `window.$message` / `$dialog` / `$notification`. Do not mount a second discrete API. Do not call `createDiscreteApi` inside `setup()`. Discrete API ignores in-app `n-xxx-provider`; do not mix with `useMessage` in the same app.

## kebab vs Pascal

Templates use kebab tags (`n-select`, `n-data-table`). `setup()` / `h()` uses Pascal (`NButton`) from `window.naive` in IIFE apps.

## n-data-table remote

For server paging, set `remote` on `n-data-table` (StackChap `RemoteDataTable` / `useListData` limit/offset). Do not paste a second remote table.

## createDiscreteApi `includes` omits `'modal'` at v2.40.4

At pin **v2.40.4** the published `includes` union is `Array<'message' | 'dialog' | 'notification' | 'loadingBar'>` — **no `'modal'`** — even though the return type and `options` still have `modal` / `modalProviderProps` and the prose lists `useModal`. Do not add `'modal'` to `includes`; IIFE agents would emit an include the pin’s types do not list.
