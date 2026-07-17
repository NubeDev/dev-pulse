#!/bin/sh
# coolify-entrypoint.sh — bootstraps the Coolify container running
# both Caddy (static + reverse proxy) and the dev-pulse Rust backend.
#
# Fork of fly-entrypoint.sh with the bundled-Postgres machinery
# removed (COOLIFY-SCOPE.md §2.3) — Postgres is a Coolify-managed
# resource, DATABASE_URL points at it directly.
#
# Responsibilities:
#   1. Materialize /etc/dev-pulse/config.toml from the template, with
#      DATABASE_URL / DP_* env vars substituted in.
#   2. Run any sub-command we were given (default: `serve`).
#      - `serve`   → run migrations, then exec Caddy in the
#                    background, dev-pulse in the foreground.
#                    Container exit = backend exit.
#      - anything else → exec dev-pulse with the given args (used for
#                    one-off admin commands via `docker exec`).
#
# Secrets are NOT substituted here — the dev-pulse binary resolves
# `secret://NAME` handles by reading env vars itself.
set -eu

TEMPLATE=/etc/dev-pulse/config.template.toml
RUNTIME=/etc/dev-pulse/config.toml

: "${DP_BIND_ADDR:=127.0.0.1:8731}"
: "${DP_PUBLIC_BASE_URL:=http://localhost:8080}"
: "${DP_DEFAULT_RETURN:=/}"
: "${DP_AUTH_SQLITE_URL:=sqlite:/data/auth.db?mode=rwc}"
: "${DP_SCHEDULER_ENABLE:=false}"
: "${DP_GITHUB_ALLOW_ORGS:=[]}"
: "${DP_GITHUB_OAUTH_CLIENT_ID:=}"

# DATABASE_URL has no local default — it must point at the Coolify
# Postgres resource. Fail fast rather than silently trying loopback.
: "${DATABASE_URL:?DATABASE_URL must be set to the Coolify Postgres resource's connection string}"

export DATABASE_URL DP_BIND_ADDR DP_PUBLIC_BASE_URL DP_DEFAULT_RETURN \
       DP_AUTH_SQLITE_URL DP_SCHEDULER_ENABLE DP_GITHUB_ALLOW_ORGS \
       DP_GITHUB_OAUTH_CLIENT_ID

mkdir -p "$(dirname "$RUNTIME")" /data
envsubst < "$TEMPLATE" > "$RUNTIME"

cmd="${1:-serve}"
shift 2>/dev/null || true

case "$cmd" in
  serve)
    echo "[entrypoint] running migrations"
    cd /app
    /usr/local/bin/dev-pulse migrate --config "$RUNTIME"

    echo "[entrypoint] starting Caddy on :8080"
    caddy run --config /etc/caddy/Caddyfile --adapter caddyfile &
    caddy_pid=$!
    trap 'kill -TERM "$caddy_pid" 2>/dev/null || true' INT TERM

    echo "[entrypoint] starting dev-pulse on $DP_BIND_ADDR"
    # The binary loads its authz policy via a hard-coded RELATIVE path
    # (crates/dp-server/policy/dev-pulse.toml). The image stages the
    # policy under /app/crates/dp-server/policy/ — cd there before exec.
    exec /usr/local/bin/dev-pulse serve --config "$RUNTIME"
    ;;
  *)
    # Other subcommands (create-admin, import-my-orgs, …). No bundled
    # PG to race with — this is the whole point of §2.3.
    cd /app
    exec /usr/local/bin/dev-pulse "$cmd" --config "$RUNTIME" "$@"
    ;;
esac
