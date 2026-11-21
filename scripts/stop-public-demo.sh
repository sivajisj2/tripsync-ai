#!/usr/bin/env bash
set -Eeuo pipefail
repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"
runtime_dir="$repo_dir/.public-demo"
pid_file="$runtime_dir/cloudflared.pid"
tmux_session="tripsync-quick-tunnel"

if command -v tmux >/dev/null 2>&1; then
  tmux kill-session -t "$tmux_session" 2>/dev/null || true
fi

if [[ -f "$pid_file" ]]; then
  pid="$(<"$pid_file")"
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
  fi
  rm -f "$pid_file"
fi

docker compose down
echo "TripSync containers stopped. The named SQLite volume was preserved."
