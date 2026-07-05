#!/usr/bin/env bash
set -eu
BIN="$(cd "$(dirname "$0")" && pwd)/target/release/arcglyph"
LOG=/tmp/arcglyph.log

pkill -x arcglyph 2>/dev/null || true
sleep 0.2
: > "$LOG"

echo "== arcglyph built $(stat -c %y "$BIN")" | tee -a "$LOG"
exec "$BIN" 2>&1 | tee -a "$LOG"
