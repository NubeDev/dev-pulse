#!/bin/sh
# fly-entrypoint.sh — bootstraps a Fly Machine running both Caddy
# (static + reverse proxy) and the dev-pulse Rust backend.
#
# Responsibilities:
#   1. Materialize /etc/dev-pulse/config.toml from the template, with
#      DATABASE_URL / DP_* env vars substituted in.
#   2. Run any sub-command we were given (default: `serve`).
#      - `serve`   → exec Caddy in the background, dev-pulse in the
#                    foreground. Container exit = backend exit.
#      - anything else → exec dev-pulse with the given args (used by
#                    fly.toml's release_command for `migrate`).
#
# Secrets are NOT substituted here — the dev-pulse binary resolves
# `secret://NAME` handles by reading env vars itself.
set -eu

TEMPLATE=/etc/dev-pulse/config.template.toml
RUNTIME=/etc/dev-pulse/config.toml

: "${DATABASE_URL:?DATABASE_URL is required (attach Fly MPG: fly mpg attach)}"
: "${DP_BIND_ADDR:=127.0.0.1:8731}"
: "${DP_PUBLIC_BASE_URL:=http://localhost:8080}"
: "${DP_DEFAULT_RETURN:=/}"
: "${DP_AUTH_SQLITE_URL:=sqlite:/data/auth.db?mode=rwc}"
: "${DP_SCHEDULER_ENABLE:=false}"
: "${DP_GITHUB_ALLOW_ORGS:=[]}"
: "${DP_GITHUB_OAUTH_CLIENT_ID:=}"

export DATABASE_URL DP_BIND_ADDR DP_PUBLIC_BASE_URL DP_DEFAULT_RETURN \
       DP_AUTH_SQLITE_URL DP_SCHEDULER_ENABLE DP_GITHUB_ALLOW_ORGS \
       DP_GITHUB_OAUTH_CLIENT_ID

mkdir -p "$(dirname "$RUNTIME")" /data
envsubst < "$TEMPLATE" > "$RUNTIME"

cmd="${1:-serve}"
shift 2>/dev/null || true

case "$cmd" in
  serve)
    echo "[entrypoint] starting Caddy on :8080"
    caddy run --config /etc/caddy/Caddyfile --adapter caddyfile &
    caddy_pid=$!
    # If Caddy dies, take the Machine down so Fly restarts cleanly.
    trap 'kill -TERM "$caddy_pid" 2>/dev/null || true' INT TERM
    echo "[entrypoint] starting dev-pulse on $DP_BIND_ADDR"
    # The binary loads its authz policy via a hard-coded RELATIVE path
    # (crates/dp-server/policy/dev-pulse.toml). The image stages the
    # policy under /app/crates/dp-server/policy/ — cd there before exec.
    cd /app
    exec /usr/local/bin/dev-pulse serve --config "$RUNTIME"
    ;;
  *)
    cd /app
    exec /usr/local/bin/dev-pulse "$cmd" --config "$RUNTIME" "$@"
    ;;
esac
