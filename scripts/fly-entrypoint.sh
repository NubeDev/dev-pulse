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

# Bundled Postgres lives on the persistent /data volume.
PGDATA=/data/pgdata
PGUSER_APP=devpulse
PGDB_APP=devpulse
PGPASS_APP="${POSTGRES_PASSWORD:-devpulse}"

: "${DATABASE_URL:=postgres://${PGUSER_APP}:${PGPASS_APP}@127.0.0.1:5432/${PGDB_APP}}"
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

start_postgres() {
  # /data is owned by root on first boot; hand it to the postgres user.
  mkdir -p "$PGDATA"
  chown -R postgres:postgres /data
  chmod 700 "$PGDATA"

  if [ ! -s "$PGDATA/PG_VERSION" ]; then
    echo "[entrypoint] initdb in $PGDATA"
    su postgres -c "/usr/lib/postgresql/16/bin/initdb \
        --pgdata='$PGDATA' --encoding=UTF8 --auth-local=trust --auth-host=md5"
    # Bind to loopback only — Caddy + app are in the same machine.
    echo "listen_addresses = '127.0.0.1'" >> "$PGDATA/postgresql.conf"
    echo "unix_socket_directories = '/tmp'"   >> "$PGDATA/postgresql.conf"
  fi

  echo "[entrypoint] starting postgres"
  su postgres -c "/usr/lib/postgresql/16/bin/pg_ctl \
      -D '$PGDATA' -l /data/postgres.log \
      -o '-c listen_addresses=127.0.0.1 -c unix_socket_directories=/tmp' \
      -w start"

  # Ensure app role + db exist (idempotent).
  su postgres -c "psql -h /tmp -tAc \
      \"SELECT 1 FROM pg_roles WHERE rolname='$PGUSER_APP'\"" \
    | grep -q 1 || \
    su postgres -c "psql -h /tmp -c \
      \"CREATE ROLE $PGUSER_APP LOGIN PASSWORD '$PGPASS_APP'\""
  su postgres -c "psql -h /tmp -tAc \
      \"SELECT 1 FROM pg_database WHERE datname='$PGDB_APP'\"" \
    | grep -q 1 || \
    su postgres -c "createdb -h /tmp -O '$PGUSER_APP' '$PGDB_APP'"
}

stop_postgres() {
  su postgres -c "/usr/lib/postgresql/16/bin/pg_ctl -D '$PGDATA' -m fast -w stop" \
    2>/dev/null || true
}

cmd="${1:-serve}"
shift 2>/dev/null || true

case "$cmd" in
  serve)
    start_postgres

    echo "[entrypoint] running migrations"
    cd /app
    /usr/local/bin/dev-pulse migrate --config "$RUNTIME"

    echo "[entrypoint] starting Caddy on :8080"
    caddy run --config /etc/caddy/Caddyfile --adapter caddyfile &
    caddy_pid=$!
    trap 'kill -TERM "$caddy_pid" 2>/dev/null || true; stop_postgres' INT TERM

    echo "[entrypoint] starting dev-pulse on $DP_BIND_ADDR"
    # The binary loads its authz policy via a hard-coded RELATIVE path
    # (crates/dp-server/policy/dev-pulse.toml). The image stages the
    # policy under /app/crates/dp-server/policy/ — cd there before exec.
    exec /usr/local/bin/dev-pulse serve --config "$RUNTIME"
    ;;
  psql)
    start_postgres
    exec su postgres -c "psql -h /tmp '$PGDB_APP'"
    ;;
  *)
    # Other subcommands (migrate, create-admin, …) need Postgres up too.
    start_postgres
    cd /app
    set +e
    /usr/local/bin/dev-pulse "$cmd" --config "$RUNTIME" "$@"
    rc=$?
    stop_postgres
    exit $rc
    ;;
esac
