#!/usr/bin/env bash
# Protocol smoke test: initialize + tools/list. No network.
set -euo pipefail
cd "$(dirname "$0")/.."

CACHE="${PWD}/target/stdio-test-cache"
mkdir -p "$CACHE"
chmod 755 "$CACHE" 2>/dev/null || true

OUT="${PWD}/target/stdio-test.out"
ERR="${PWD}/target/stdio-test.err"
mkdir -p target

NAIVE_UI_MCP_CACHE="$CACHE" cargo run --quiet >"$OUT" 2>"$ERR" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
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

for tool in naive_sync naive_list naive_search naive_component naive_prop naive_demo naive_theme naive_discrete naive_get; do
  absent "$tool" "\"name\"[[:space:]]*:[[:space:]]*\"${tool}\""
done

if [[ "$fail" -ne 0 ]]; then
  echo "stdout:"
  cat "$OUT"
  echo "stderr:"
  cat "$ERR"
  exit 1
fi
echo "All stdio checks passed."
