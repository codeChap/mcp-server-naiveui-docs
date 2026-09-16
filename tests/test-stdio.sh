#!/usr/bin/env bash
# Protocol smoke test: initialize + tools/list (all ten) + naive_status +
# naive_search + naive_component against the fixture cache. No network.
# 2>/dev/null is only for the server's tracing (stderr); assertions read stdout.
set -euo pipefail
cd "$(dirname "$0")/.."

CACHE="${PWD}/tests/fixtures/tree"
chmod 755 tests/fixtures "$CACHE" || true

cargo build --quiet --bin mcp-server-naive-ui

BIN="${PWD}/target/debug/mcp-server-naive-ui"
OUT="${PWD}/target/stdio-test.out"
mkdir -p target

env -u NAIVE_UI_MCP_REV -u NAIVE_UI_MCP_SYNC_ON_START \
  NAIVE_UI_MCP_CACHE="$CACHE" "$BIN" >"$OUT" 2>/dev/null <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"naive_status","arguments":{}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"naive_search","arguments":{"query":"remote"}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"naive_component","arguments":{"name":"button"}}}
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"naive_list","arguments":{}}}
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"naive_demo","arguments":{"component":"button","name":"basic"}}}
{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"naive_demo","arguments":{"component":"button","name":"../x"}}}
{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"naive_theme","arguments":{"component":"button"}}}
{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"naive_discrete","arguments":{}}}
{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"naive_get","arguments":{"id":"gotchas"}}}
{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"naive_component","arguments":{"name":"discrete"}}}
EOF

fail=0
check() {
  if grep -qE "$2" "$OUT"; then
    echo "  ok   — $1"
  else
    echo "  FAIL — $1"
    fail=1
  fi
}

echo "stdio protocol:"
check "initialize server name naive-ui" '"name"[[:space:]]*:[[:space:]]*"naive-ui"'
check "tools/list includes naive_status" '"name"[[:space:]]*:[[:space:]]*"naive_status"'
check "tools/list includes naive_sync" '"name"[[:space:]]*:[[:space:]]*"naive_sync"'
check "tools/list includes naive_list" '"name"[[:space:]]*:[[:space:]]*"naive_list"'
check "tools/list includes naive_search" '"name"[[:space:]]*:[[:space:]]*"naive_search"'
check "tools/list includes naive_component" '"name"[[:space:]]*:[[:space:]]*"naive_component"'
check "tools/list includes naive_prop" '"name"[[:space:]]*:[[:space:]]*"naive_prop"'
check "tools/list includes naive_demo" '"name"[[:space:]]*:[[:space:]]*"naive_demo"'
check "tools/list includes naive_get" '"name"[[:space:]]*:[[:space:]]*"naive_get"'
check "tools/list includes naive_theme" '"name"[[:space:]]*:[[:space:]]*"naive_theme"'
check "tools/list includes naive_discrete" '"name"[[:space:]]*:[[:space:]]*"naive_discrete"'

python3 - "$OUT" <<'PY' || fail=1
import json, sys

path = sys.argv[1]
with open(path, encoding="utf-8") as f:
    raw = f.read()

decoder = json.JSONDecoder()
messages = {}
idx = 0
while idx < len(raw):
    while idx < len(raw) and raw[idx].isspace():
        idx += 1
    if idx >= len(raw):
        break
    if raw[idx] != "{":
        nxt = raw.find("{", idx)
        if nxt < 0:
            break
        idx = nxt
        continue
    try:
        obj, end = decoder.raw_decode(raw, idx)
    except json.JSONDecodeError:
        break
    idx = end
    if isinstance(obj, dict) and "id" in obj:
        messages[obj["id"]] = obj

def content_text(msg):
    result = msg.get("result") or {}
    content = result.get("content") or []
    if not content:
        return ""
    return content[0].get("text") or ""

def is_error(msg):
    result = msg.get("result") or {}
    return bool(result.get("isError")) or "error" in msg

fail = 0

def ok(label):
    print(f"  ok   — {label}")

def bad(label, extra=""):
    global fail
    fail = 1
    print(f"  FAIL — {label}{extra}")

def load_payload(msg_id, label):
    msg = messages.get(msg_id)
    if not msg:
        bad(f"{label} response missing")
        return None
    text = content_text(msg)
    try:
        return json.loads(text)
    except json.JSONDecodeError as e:
        bad(f"{label} JSON.parse", f": {e}")
        return None

# tools/list names (id=2)
listed = messages.get(2)
if not listed:
    bad("tools/list response missing")
    sys.exit(1)
tools = ((listed.get("result") or {}).get("tools")) or []
names = {t.get("name") for t in tools}
want = {
    "naive_status",
    "naive_sync",
    "naive_list",
    "naive_search",
    "naive_component",
    "naive_prop",
    "naive_demo",
    "naive_get",
    "naive_theme",
    "naive_discrete",
}
missing = sorted(want - names)
extra = sorted(names - want)
if missing or extra:
    bad("tools/list names", f" missing {missing} extra {extra}")
else:
    ok("tools/list all ten names")

# naive_status (id=3)
status = load_payload(3, "naive_status")
if status is not None:
    pin = status.get("pin")
    pages = status.get("pages") or 0
    missing_src = status.get("missing") or []
    if pin == "v2.40.4" and pages >= 7 and "naive-ui" not in missing_src:
        ok("naive_status pin v2.40.4, catalog loaded")
    else:
        bad("naive_status payload", f": {json.dumps(status)[:400]}")

# naive_search remote → data-table (id=4)
search = load_payload(4, "naive_search")
if search is not None:
    hits = search.get("hits") or []
    first = hits[0] if hits else {}
    snippet = first.get("snippet") or ""
    if first.get("id") == "data-table" and "remote" in snippet:
        ok("naive_search remote hits data-table")
    else:
        bad("naive_search remote", f": {json.dumps(search)[:400]}")

# naive_component JSON contains attr-type and parses (id=5)
comp = load_payload(5, "naive_component")
if comp is not None:
    dump = json.dumps(comp)
    if "attr-type" in dump:
        ok("naive_component JSON contains attr-type and JSON.parse-able")
    else:
        bad("naive_component attr-type", f": {dump[:400]}")

# naive_list returns button (id=6)
lst = load_payload(6, "naive_list")
if lst is not None:
    blob = json.dumps(lst)
    if '"id": "button"' in blob or any(
        (r.get("id") == "button") for r in (lst.get("rows") or [])
    ):
        ok("naive_list returns button")
    else:
        bad("naive_list returns button", f": {blob[:400]}")

# naive_demo returns fixture vue (id=7)
demo = load_payload(7, "naive_demo")
if demo is not None:
    body = demo.get("body") or ""
    if "n-button" in body or "<n-button>" in body:
        ok("naive_demo returns fixture vue")
    else:
        bad("naive_demo fixture vue", f": {json.dumps(demo)[:400]}")

# traversal name=../x errors (id=8)
trav = messages.get(8)
if not trav:
    bad("naive_demo traversal response missing")
else:
    text = content_text(trav)
    if is_error(trav) or "traversal" in text.lower() or "invalid" in text.lower():
        ok("traversal name=../x errors")
    else:
        bad("traversal name=../x errors", f": {text[:400]}")

# naive_theme button css vars (id=9)
theme = load_payload(9, "naive_theme")
if theme is not None:
    vars_ = theme.get("css_vars") or []
    if "--n-text-color" in vars_ and "--n-border-color-xxx" not in vars_:
        ok("naive_theme button has --n-text-color not xxx")
    else:
        bad("naive_theme css vars", f": {json.dumps(theme)[:400]}")

# naive_discrete (id=10)
disc = load_payload(10, "naive_discrete")
if disc is not None:
    dump = json.dumps(disc)
    sig = disc.get("signature_ts") or ""
    if "createDiscreteApi" in dump and "useMessage" in dump and "|'modal'|" not in sig:
        ok("naive_discrete createDiscreteApi / useMessage, no |'modal'| in signature")
    else:
        bad("naive_discrete payload", f": {dump[:400]}")

# naive_get gotchas (id=11)
got = load_payload(11, "naive_get gotchas")
if got is not None:
    if got.get("id") == "gotchas":
        ok("gotchas id is gotchas")
    else:
        bad("gotchas id", f": {json.dumps(got)[:400]}")

# naive_component discrete (id=12)
compd = load_payload(12, "naive_component discrete")
if compd is not None:
    if compd.get("id") == "discrete" and "createDiscreteApi" in json.dumps(compd):
        ok("naive_component(discrete) still works")
    else:
        bad("naive_component discrete", f": {json.dumps(compd)[:400]}")

sys.exit(fail)
PY

if [[ "$fail" -ne 0 ]]; then
  echo "stdout:"
  cat "$OUT"
  exit 1
fi
echo "All stdio checks passed."
