#!/usr/bin/env bash
set -Eeuo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"
runtime_dir="$repo_dir/.public-demo"
mkdir -p "$runtime_dir"
log_file="$runtime_dir/cloudflared.log"
pid_file="$runtime_dir/cloudflared.pid"
tmux_session="tripsync-quick-tunnel"
compose_env="$runtime_dir/compose.env"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required" >&2
  exit 1
fi

if [[ -f apps/api/.env ]]; then
  set -a
  # shellcheck disable=SC1091
  source apps/api/.env
  set +a
fi

if [[ -z "${SERPAPI_KEY:-}" ]]; then
  echo "SERPAPI_KEY is not set; add it to apps/api/.env or the environment" >&2
  exit 1
fi

if [[ -z "${NEXCONN_APP_KEY:-}" && -n "${NEXT_PUBLIC_NEXCONN_APP_KEY:-}" ]]; then
  export NEXCONN_APP_KEY="$NEXT_PUBLIC_NEXCONN_APP_KEY"
fi

export APP_ENV=production
export COOKIE_SECURE=true
export NEXT_PUBLIC_API_URL=""
export APP_ORIGIN="https://pending-quick-tunnel.trycloudflare.com"

# Compose also reads a repository .env when one exists. Use a private,
# generated env file so the dynamic public origin always wins without
# displaying or committing server-only credentials.
{
  printf 'DOMAIN=%s\n' "${DOMAIN:-}"
  printf 'APP_ORIGIN=%s\n' "$APP_ORIGIN"
  printf 'NEXT_PUBLIC_API_URL=%s\n' "$NEXT_PUBLIC_API_URL"
  printf 'COOKIE_SECURE=%s\n' "$COOKIE_SECURE"
  printf 'APP_ENV=%s\n' "$APP_ENV"
  printf 'SERPAPI_KEY=%s\n' "$SERPAPI_KEY"
  printf 'NEXCONN_APP_KEY=%s\n' "${NEXCONN_APP_KEY:-}"
  printf 'NEXCONN_APP_SECRET=%s\n' "${NEXCONN_APP_SECRET:-}"
  printf 'NEXCONN_API_BASE_URL=%s\n' "${NEXCONN_API_BASE_URL:-https://api.sg-light-api.com}"
  printf 'SESSION_TTL_SECONDS=%s\n' "${SESSION_TTL_SECONDS:-604800}"
} >"$compose_env"
compose=(docker compose --env-file "$compose_env")

"${compose[@]}" up -d --build

cloudflared_bin="$(command -v cloudflared || true)"
if [[ -z "$cloudflared_bin" && -x "$repo_dir/.tools/cloudflared" ]]; then
  cloudflared_bin="$repo_dir/.tools/cloudflared"
fi
if [[ -z "$cloudflared_bin" ]]; then
  echo "cloudflared is not installed. Install it from https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/" >&2
  exit 1
fi

public_url=""
if [[ -f "$pid_file" ]] && kill -0 "$(<"$pid_file")" 2>/dev/null && [[ -s "$runtime_dir/url" ]]; then
  public_url="$(<"$runtime_dir/url")"
else
  rm -f "$log_file" "$runtime_dir/url"
  if command -v tmux >/dev/null 2>&1; then
    tmux kill-session -t "$tmux_session" 2>/dev/null || true
    tmux new-session -d -s "$tmux_session" "exec '$cloudflared_bin' tunnel --protocol http2 --url http://localhost:3000 >'$log_file' 2>&1"
    tmux_pid="$(tmux list-panes -t "$tmux_session" -F '#{pane_pid}' | head -n1)"
    echo "$tmux_pid" >"$pid_file"
  else
    setsid "$cloudflared_bin" tunnel --protocol http2 --url http://localhost:3000 >"$log_file" 2>&1 < /dev/null &
    echo $! >"$pid_file"
  fi

  for _ in {1..60}; do
    public_url="$(sed -nE 's#.*(https://[a-z0-9-]+\.trycloudflare\.com).*#\1#p' "$log_file" | head -n1 || true)"
    [[ -n "$public_url" ]] && break
    sleep 1
  done

  if [[ -z "$public_url" ]]; then
    echo "Quick Tunnel did not report a public URL; inspect $log_file" >&2
    exit 1
  fi

  printf '%s\n' "$public_url" >"$runtime_dir/url"
fi
export APP_ORIGIN="$public_url"
printf 'APP_ORIGIN=%s\n' "$APP_ORIGIN" >"$compose_env"
# Re-append the unchanged private/runtime values after replacing the origin.
printf '%s\n' "NEXT_PUBLIC_API_URL=$NEXT_PUBLIC_API_URL" "COOKIE_SECURE=$COOKIE_SECURE" "APP_ENV=$APP_ENV" "SERPAPI_KEY=$SERPAPI_KEY" "NEXCONN_APP_KEY=${NEXCONN_APP_KEY:-}" "NEXCONN_APP_SECRET=${NEXCONN_APP_SECRET:-}" "NEXCONN_API_BASE_URL=${NEXCONN_API_BASE_URL:-https://api.sg-light-api.com}" "SESSION_TTL_SECONDS=${SESSION_TTL_SECONDS:-604800}" >>"$compose_env"
"${compose[@]}" up -d --no-deps api

echo "TripSync public demo: $public_url"
echo "Tunnel log: $log_file"
