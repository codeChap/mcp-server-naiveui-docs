#!/usr/bin/env bash
# Protocol smoke test: initialize + tools/list + locator/reader tools. No network.
set -euo pipefail
cd "$(dirname "$0")/.."

CACHE="${PWD}/tests/fixtures/tree"
chmod 755 tests/fixtures "$CACHE" 2>/dev/null || true

OUT="${PWD}/target/stdio-test.out"
ERR="${PWD}/target/stdio-test.err"
mkdir -p target

env -u NAIVE_UI_MCP_REV -u NAIVE_UI_MCP_SYNC_ON_START \
  NAIVE_UI_MCP_CACHE="$CACHE" cargo run --quiet >"$OUT" 2>"$ERR" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"naive_status","arguments":{}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"naive_list","arguments":{}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"naive_component","arguments":{"name":"button"}}}
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"naive_demo","arguments":{"component":"button","name":"basic"}}}
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"naive_demo","arguments":{"component":"button","name":"../x"}}}
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
absent() {
  if grep -qE "$2" "$OUT"; then
    echo "  FAIL — $1 (should be absent)"
    fail=1
  else
    echo "  ok   — $1 absent"
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

for tool in naive_theme naive_discrete; do
  absent "$tool" "\"name\"[[:space:]]*:[[:space:]]*\"${tool}\""
done

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
}
missing = sorted(want - names)
extra = sorted(names & {"naive_discrete", "naive_theme"})
if missing:
    bad("tools/list names", f" missing {missing}")
else:
    ok("tools/list names for this PR")
if extra:
    bad("tools/list extra", f" {extra}")
else:
    ok("tools/list has no naive_discrete / naive_theme")

# naive_list returns button (id=4)
lst = messages.get(4)
if not lst:
    bad("naive_list response missing")
else:
    text = content_text(lst)
    try:
        payload = json.loads(text)
    except json.JSONDecodeError as e:
        bad("naive_list JSON.parse", f": {e}")
        payload = None
    if payload is not None:
        blob = json.dumps(payload)
        if '"id": "button"' in blob or any(
            (r.get("id") == "button") for r in (payload.get("rows") or [])
        ):
            ok("naive_list returns button")
        else:
            bad("naive_list returns button", f": {blob[:400]}")

# naive_component JSON contains attr-type and parses (id=5)
comp = messages.get(5)
if not comp:
    bad("naive_component response missing")
else:
    text = content_text(comp)
    try:
        payload = json.loads(text)
    except json.JSONDecodeError as e:
        bad("naive_component JSON.parse", f": {e}")
        payload = None
    if payload is not None:
        dump = json.dumps(payload)
        if "attr-type" in dump:
            ok("naive_component JSON contains attr-type and JSON.parse-able")
        else:
            bad("naive_component attr-type", f": {dump[:400]}")

# naive_demo returns fixture vue (id=6)
demo = messages.get(6)
if not demo:
    bad("naive_demo response missing")
else:
    text = content_text(demo)
    try:
        payload = json.loads(text)
    except json.JSONDecodeError as e:
        bad("naive_demo JSON.parse", f": {e}")
        payload = None
    if payload is not None:
        body = payload.get("body") or ""
        if "n-button" in body or "<n-button>" in body:
            ok("naive_demo returns fixture vue")
        else:
            bad("naive_demo fixture vue", f": {json.dumps(payload)[:400]}")

# traversal name=../x errors (id=7)
trav = messages.get(7)
if not trav:
    bad("naive_demo traversal response missing")
else:
    text = content_text(trav)
    if is_error(trav) or "traversal" in text.lower() or "invalid" in text.lower():
        ok("traversal name=../x errors")
    else:
        bad("traversal name=../x errors", f": {text[:400]}")

sys.exit(fail)
PY

if [[ "$fail" -ne 0 ]]; then
  echo "stdout:"
  cat "$OUT"
  echo "stderr:"
  cat "$ERR"
  exit 1
fi
echo "All stdio checks passed."
