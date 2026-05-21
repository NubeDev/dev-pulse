# Deploy dev-pulse to Fly.io

End-to-end runbook for deploying dev-pulse to Fly.io. Battle-tested
against the canonical `dev-pulse` app in region `cdg` reachable at
**<https://dev-pulse.fly.dev>**.

All commands run from `/home/user/code/rust/dev-pulse` unless noted.

---

## What gets deployed

A **single Fly Machine** runs three processes in one container under
[tini(1)](https://github.com/krallin/tini):

| # | Process | Port | Role |
|---|---|---|---|
| 1 | **Postgres 16** | 127.0.0.1:5432 | App database (`devpulse` DB). Data lives on the persistent `/data` volume at `/data/pgdata`. |
| 2 | **Caddy 2** | 0.0.0.0:8080 | Static SPA + reverse proxy. Fronts everything from the runtime container. |
| 3 | **dev-pulse** | 127.0.0.1:8731 | Rust backend (axum + dp-server). Owns Postgres, the sqlite auth sidecar at `/data/auth.db`, OAuth + webhook surfaces, and the scheduler. |

Fly's **fly-proxy** terminates TLS on 80/443 and forwards plain HTTP
to Caddy on `:8080`. Caddy is **not** the TLS terminator (`auto_https
off`); it serves the SPA bundle and reverse-proxies all non-static
paths to the backend.

> **Why bundled Postgres?** Fly Managed Postgres ("MPG") isn't
> available in `cdg`, and a dedicated MPG cluster + attached app
> ~doubles the monthly cost vs. running a single shared-cpu-1x VM
> with PG on its own volume. The trade-off is that there is no
> redundant DB instance — back the `/data` volume up if you care.

### Topology

```
                    https://dev-pulse.fly.dev/
                              │
                    ┌─────────▼──────────────┐
                    │ fly-proxy (TLS)        │
                    │ shared v4 + ded v6     │
                    └─────────┬──────────────┘
                              │ plain HTTP, internal :8080
                    ┌─────────▼──────────────┐
                    │ Caddy                  │
                    │ - serves /, /index.html│
                    │   /assets/* (SPA)      │
                    │ - reverse_proxy        │
                    │   EVERYTHING ELSE →    │
                    │   127.0.0.1:8731       │
                    └────────┬───────────────┘
                             │
                  ┌──────────▼──────────────┐
                  │ dev-pulse backend       │
                  │ 127.0.0.1:8731          │
                  └─┬───────────────────┬───┘
                    │                   │
       ┌────────────▼─────────┐  ┌──────▼──────────────┐
       │ Postgres 16          │  │ /data/auth.db       │
       │ 127.0.0.1:5432       │  │ (sqlite — sessions, │
       │ data → /data/pgdata  │  │  oauth identities)  │
       └──────────────────────┘  └─────────────────────┘
                 │                          │
                 └───── single Fly volume `dev_pulse_data` ─────┘
                                   /data
```

### SPA routing

The frontend uses **hash routing** (`/#/account/settings`), so the
only paths the server ever sees as "SPA" are `/`, `/index.html`,
`/assets/*`, and a few top-level static files (`/favicon.ico`,
`/robots.txt`, `/manifest.json`, `/static/*`, `/icons/*`).

The Caddyfile therefore matches those explicitly and **falls back to
the backend for everything else** (allow-all reverse proxy). This is
the maintainable choice — every time a new API route lands, we don't
have to remember to add a prefix to the proxy table.

---

## Prerequisites

1. **flyctl**:
   ```bash
   curl -L https://fly.io/install.sh | sh
   export PATH="$HOME/.fly/bin:$PATH"
   fly auth login
   fly auth whoami
   ```
2. **Local HTTPS pre-flight passes** — see
   [HTTPS-LOCAL-TEST.md](HTTPS-LOCAL-TEST.md). Iterating against
   `https://localhost` is ~30 s; against Fly it's ~5–10 min per try.
3. **Build context** = `/home/user/code/rust/` (parent of `dev-pulse/`
   and `starter/`). Workspace path-deps reference `../starter`;
   `make fly-deploy` cds up for you.

---

## Step 0: Prove it works locally with HTTPS

```bash
# One-time
sudo apt install -y mkcert libnss3-tools
mkcert -install

# Bring up the canonical image behind mkcert TLS on https://localhost
make fly-local

# Iterate. When green, tear down (keeps volumes):
make fly-local-down
```

Don't skip this. The local harness runs the **same image** Fly will,
behind real TLS, with the same env-var wiring.

---

## Step 1: Create the Fly app + volume (first time only)

```bash
APP=dev-pulse
REGION=cdg

# Reserve the app name (does NOT deploy).
fly apps create "$APP"

# Persistent volume — holds /data/pgdata AND /data/auth.db.
fly volumes create dev_pulse_data --region "$REGION" --size 1 -a "$APP" --yes

# Optional: pin a dedicated v6 (free) and v4 if you need it ($).
fly ips list -a "$APP"
# fly ips allocate-v4 -a "$APP"
# fly ips allocate-v6 -a "$APP"
```

Postgres is bundled in the image — there's no `fly mpg attach` step.

---

## Step 2: Set secrets

The fetcher reads its PAT and webhook secret from env. `envsubst`
substitutes `${VAR}` placeholders into the runtime TOML config.

```bash
APP=dev-pulse

fly secrets set -a "$APP" \
  GITHUB_PAT="ghp_…"                                       \
  GITHUB_WEBHOOK_SECRET="$(openssl rand -hex 32)"          \
  OAUTH_GITHUB_CLIENT_SECRET="<github-oauth-app-secret>"   \
  DP_GITHUB_OAUTH_CLIENT_ID="Iv1.…"                        \
  POSTGRES_PASSWORD="$(openssl rand -hex 24)"

# DP_GITHUB_ALLOW_ORGS is a TOML JSON array — quote it as ONE arg,
# single-quoted so the shell doesn't strip the inner double-quotes.
# Setting it wrong here cost us ~1h of "Error: parse config TOML".
fly secrets set -a "$APP" \
  'DP_GITHUB_ALLOW_ORGS=["NubeIO","NubeDev","PJNube"]'

# Sanity check (values are hidden, but names + digests are listed):
fly secrets list -a "$APP"
```

| Secret | Source | Notes |
|---|---|---|
| `GITHUB_PAT` | classic / fine-grained PAT | Read scopes: `repo:status, public_repo, read:org, read:user`. |
| `GITHUB_WEBHOOK_SECRET` | `openssl rand -hex 32` | Paste the same value into the GitHub App webhook settings. |
| `OAUTH_GITHUB_CLIENT_SECRET` | GitHub OAuth App | Client secret. |
| `DP_GITHUB_OAUTH_CLIENT_ID` | GitHub OAuth App | Not secret strictly, but env-driven so the same image works for multiple deploys. |
| `DP_GITHUB_ALLOW_ORGS` | operator | **Must be valid JSON.** `envsubst` writes it verbatim into the TOML. |
| `POSTGRES_PASSWORD` | `openssl rand -hex 24` | Password the entrypoint uses when creating the bundled-PG role. If unset, defaults to `devpulse` — fine for personal deploys, change it before sharing the app. |

> ⚠️ **Shell quoting gotcha** — `DP_GITHUB_ALLOW_ORGS=[...]` must be
> a single quoted arg. The form
> ```bash
> fly secrets set DP_GITHUB_ALLOW_ORGS=["a","b"]   # WRONG
> ```
> loses the double-quotes during expansion, the resulting TOML
> becomes `allow_orgs = [a,b]`, and the binary crash-loops with
> "TOML parse error … invalid array". Always single-quote the whole
> `KEY=VALUE`.

### GitHub OAuth App config

In the OAuth App settings on GitHub:

- **Homepage URL** — `https://dev-pulse.fly.dev`
- **Authorization callback URL** — `https://dev-pulse.fly.dev/auth/oauth/github/callback`

For the local TLS harness, register a **separate** OAuth App with
callback `https://localhost/auth/oauth/github/callback`.

---

## Step 3: Deploy

```bash
make fly-deploy
```

This wraps:

```bash
cd /home/user/code/rust && fly deploy \
  --app dev-pulse \
  --config dev-pulse/fly.toml \
  --dockerfile dev-pulse/Dockerfile.fly \
  --ignorefile dev-pulse/.dockerignore.fly \
  --remote-only \
  .
```

**Important:** the build context is the **parent** directory (so
`../starter` path-deps resolve). Without an ignorefile that strips
`target/`, `node_modules/`, `runs/`, `.codeless/`,
`codeless-workspace/`, etc., the upload balloons to ~2.5 GB and times
out the remote builder.

Two ignorefiles exist for the same reason:

- [.dockerignore.fly](.dockerignore.fly) — passed explicitly via
  `--ignorefile`. Authoritative for Fly builds.
- [../.dockerignore](../.dockerignore) (at the parent dir) — a
  fallback in case `flyctl` ever stops honouring `--ignorefile`. Same
  content.

The first build takes ~10–15 min (Rust release + pnpm SPA + apt
install of Caddy & Postgres). Subsequent builds with cargo's
`/usr/local/cargo/registry` cache and the workspace target cache
mount take ~3–5 min.

Migrations run **inside the entrypoint**, after Postgres is up,
before Caddy and dev-pulse start. There is no `release_command` —
release VMs on Fly don't get the `/data` volume, so they can't reach
the bundled Postgres.

---

## Step 4: Seed the admin user

The frontend's login form posts to `POST /auth/login`. To use it
without going through GitHub OAuth, create a local password row in
`/data/auth.db`:

```bash
fly ssh console -a dev-pulse -C \
  "sh -c 'cd /app && /usr/local/bin/dev-pulse create-admin \
    --config /etc/dev-pulse/config.toml \
    --email dev@dev.com \
    --password dev123456789'"
```

> ⚠️ **Don't invoke the entrypoint wrapper here.** Running
> `fly-entrypoint.sh create-admin` tries to start a second Postgres
> instance and fails ("pg_ctl: another server might be running").
> Always call `/usr/local/bin/dev-pulse <subcmd>` directly when the
> main `serve` process is already running.

Verify:

```bash
curl -i -X POST https://dev-pulse.fly.dev/auth/login \
  -H 'content-type: application/json' \
  -d '{"email":"dev@dev.com","password":"dev123456789"}'
# Expect: HTTP/2 200 with `set-cookie: starter_session=…`
```

---

## Step 5: Seed orgs + repos

Out of the box, the app's `dp_orgs` / `dp_repos` tables are empty —
the fetcher only ingests data for repos that have been **explicitly
tracked**. Two CLI subcommands handle the bootstrap:

```bash
# Import every org the PAT can see.
fly ssh console -a dev-pulse -C \
  "sh -c 'cd /app && /usr/local/bin/dev-pulse import-my-orgs \
    --config /etc/dev-pulse/config.toml'"

# Import repos from a scoped list of orgs.
fly ssh console -a dev-pulse -C \
  "sh -c 'cd /app && /usr/local/bin/dev-pulse import-my-repos \
    --config /etc/dev-pulse/config.toml \
    --orgs NubeDev,NubeIO,PJNube \
    --active-within-days 365 \
    --max 200'"
```

`import-my-repos` flags:

| Flag | Default | Notes |
|---|---|---|
| `--orgs <csv>` | (none — all orgs) | Comma-separated owner-login allow-list, case-insensitive. |
| `--active-within-days <n>` | `60` | Skip repos whose `pushed_at` is older than this. `0` disables. |
| `--include-forks` | off | Forks are skipped by default. |
| `--max <n>` | `500` | Hard cap. |

### Trigger the first sync

```bash
# Log in to get a session cookie.
curl -sS -c /tmp/dp.cookies -X POST https://dev-pulse.fly.dev/auth/login \
  -H 'content-type: application/json' \
  -d '{"email":"dev@dev.com","password":"dev123456789"}'

# Kick off a reconciler tick (returns {ran, items, errors, partial}).
curl -sS -b /tmp/dp.cookies -X POST https://dev-pulse.fly.dev/admin/refresh
```

A `partial: true` response means the request budget (default
`[github].max_requests_per_run = 200`) was exhausted — run `/admin/refresh`
again, or bump the budget in `crates/dev-pulse/config.fly.toml`.

### Optional: enable the scheduler

By default `DP_SCHEDULER_ENABLE=false` and sync only happens when
`POST /admin/refresh` is called. To enable the every-5-minute tick:

```bash
fly secrets set -a dev-pulse DP_SCHEDULER_ENABLE=true
```

Keep this `false` if you scale to >1 Machine without per-machine
overrides — multiple tickers will race.

---

## Step 6: Verify

```bash
APP=dev-pulse

fly status   -a "$APP"
fly logs     -a "$APP" --no-tail | tail -50

# Public endpoints
curl -sI  "https://$APP.fly.dev/health"
curl -s   "https://$APP.fly.dev/openapi.json" | python3 -m json.tool | head
curl -sI  "https://$APP.fly.dev/" | head -3

# Shell in
fly ssh console -a "$APP"
```

Expected processes inside the container:

```
$ fly ssh console -a dev-pulse -C 'ps -eo pid,comm'
  PID COMMAND
    1 init
    …   tini
    …   fly-entrypoint
    …   postgres
    …   caddy
    …   dev-pulse
```

---

## Custom domain

```bash
APP=dev-pulse
DOMAIN=dev-pulse.example.com

fly certs add "$DOMAIN" -a "$APP"
# Follow the DNS instructions flyctl prints (A + AAAA, or CNAME).
fly certs show "$DOMAIN" -a "$APP"

# Point the backend at the new origin:
fly secrets set DP_PUBLIC_BASE_URL="https://$DOMAIN" -a "$APP"
# Update the GitHub OAuth App callback to https://$DOMAIN/auth/oauth/github/callback.
# Then re-deploy so the new config.toml is materialised:
make fly-deploy
```

---

## Key files

| File | Purpose |
|---|---|
| [fly.toml](fly.toml) | Fly app config — region, VM size, env, `[http_service]`, `[mounts]`. No `release_command` (migrations run in the entrypoint). |
| [Dockerfile.fly](Dockerfile.fly) | Multi-stage: Rust release → pnpm SPA → debian-slim runtime with Caddy + Postgres 16 + tini + gettext-base + curl. |
| [.dockerignore.fly](.dockerignore.fly) | Whitelist for the parent-dir build context. Trims ~2.5 GB → ~22 MB. |
| [Caddyfile](Caddyfile) | SPA-explicit + allow-all reverse proxy. **First match wins**: serve SPA for `/`, `/index.html`, `/assets/*`, etc., proxy everything else to `127.0.0.1:8731`. |
| [scripts/fly-entrypoint.sh](scripts/fly-entrypoint.sh) | initdb + start PG + run migrations + start Caddy + exec dev-pulse. |
| [crates/dev-pulse/config.fly.toml](crates/dev-pulse/config.fly.toml) | Runtime config **template** with `${VAR}` placeholders. Materialised at boot via `envsubst`. |
| [docker-compose.fly-local.yml](docker-compose.fly-local.yml) + [Caddyfile.local](Caddyfile.local) | Local TLS pre-flight harness — see [HTTPS-LOCAL-TEST.md](HTTPS-LOCAL-TEST.md). |
| [Makefile](Makefile) | `fly-deploy`, `fly-logs`, `fly-ssh`, `fly-status`, `fly-local*`. |

---

## Environment variables

Non-secret defaults are in [fly.toml](fly.toml); secrets via
`fly secrets set`. All are substituted into
[crates/dev-pulse/config.fly.toml](crates/dev-pulse/config.fly.toml).

| Variable | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | `postgres://devpulse:$POSTGRES_PASSWORD@127.0.0.1:5432/devpulse` | Auto-built in the entrypoint from `POSTGRES_PASSWORD` if not set. |
| `POSTGRES_PASSWORD` | `devpulse` | App-role password for bundled PG. Set as a Fly secret. |
| `DP_PUBLIC_BASE_URL` | `https://dev-pulse.fly.dev` (fly.toml) | Drives the OAuth `redirect_uri` and every absolute URL the backend emits. |
| `DP_BIND_ADDR` | `127.0.0.1:8731` | Where the backend listens inside the container. Caddy proxies here. |
| `DP_DEFAULT_RETURN` | `/` | OAuth post-callback default landing. |
| `DP_AUTH_SQLITE_URL` | `sqlite:/data/auth.db?mode=rwc` | Auth sidecar path on the volume. |
| `DP_SCHEDULER_ENABLE` | `false` | When `true` *and* `GITHUB_PAT` is set, reconciler ticks every `[scheduler].tick_interval_secs`. |
| `DP_GITHUB_OAUTH_CLIENT_ID` | (secret) | GitHub OAuth App client ID. |
| `DP_GITHUB_ALLOW_ORGS` | (secret) | JSON array of allowed GH org logins. **Must be valid JSON.** |
| `GITHUB_PAT` | (secret) | Resolves `secret://github/pat`. |
| `GITHUB_WEBHOOK_SECRET` | (secret) | Resolves `secret://github/webhook_secret`. |
| `OAUTH_GITHUB_CLIENT_SECRET` | (secret) | Resolves `secret://oauth/github_client_secret`. |
| `RUST_LOG` | `info,dev_pulse=info,dp_server=info,sqlx=warn` | Tracing filter. |

---

## Persistent volume layout (`/data`)

```
/data/
├── pgdata/         # Postgres 16 cluster (initdb'd on first boot)
├── postgres.log    # PG server log (started via pg_ctl -l)
└── auth.db         # sqlite — starter-auth-users + starter-auth-oauth row families
```

Everything in dev-pulse is on this one volume. Snapshots are
automatic (Fly's default schedule: daily, 5 retained). To force one:

```bash
fly volumes snapshots create <vol-id>
```

### Wipe recipes

- **Reset sessions only** (PG data preserved):
  ```bash
  fly ssh console -a dev-pulse -C 'rm -f /data/auth.db'
  fly machine restart -a dev-pulse <machine-id>
  ```
- **Full reset** — destroy + recreate the volume (irreversible):
  ```bash
  fly volumes list   -a dev-pulse
  fly volumes destroy <vol-id> -a dev-pulse --yes
  fly volumes create  dev_pulse_data --region cdg --size 1 -a dev-pulse --yes
  make fly-deploy
  ```

---

## Operational pitfalls we hit (and fixed)

A running log of the deploy bugs we already paid for, so we don't pay
again next time:

### 1. flyctl auto-prompts to log in mid-command
First `fly` invocation after install (or after token expiry) opens a
browser tab; if you Ctrl-C the auth flow, subsequent commands keep
re-prompting. Resolve with `fly auth login` explicitly.

### 2. Rust toolchain bumps for transitive MSRV
Several deps (`home`, `icu_*`, `time`) require Rust ≥ 1.85 once
`edition2024` was enabled. The runtime base image is
`rust:1.90-slim-bookworm` — bump it in lockstep with the workspace.

### 3. Authz policy not found at boot
`dp-server` loads `crates/dp-server/policy/dev-pulse.toml` via a
**hard-coded relative path**. The Dockerfile copies it into
`/app/crates/dp-server/policy/`, and the entrypoint does `cd /app`
before exec'ing the binary.

### 4. Caddy directive ordering ate the API
Mixing `try_files` (SPA fallback) and `reverse_proxy` (API) as bare
directives inside the same site block applies Caddy's implicit
ordering — `try_files` won, and the backend never saw `/orgs`,
`/me/settings`, etc. (the SPA's `index.html` came back instead, which
shows up in the frontend as
`SyntaxError: Unexpected token '<', "<!doctype "... is not valid JSON`).
**Fix:** use explicit `handle` blocks. They're mutually exclusive,
first-match-wins, and bypass directive ordering entirely. We further
simplified to "SPA = explicit static paths; everything else =
backend".

### 5. Build context bloat
Without `.dockerignore.fly`, the parent build context is ~2.5 GB
(local cargo `target/`, pnpm `node_modules/`, `runs/`,
`codeless-workspace/`). Fly's remote builder times out after ~5 min
of upload. The ignorefile drops it to ~22 MB.

### 6. React types duplicated in fresh installs
The frontend pins `react@18` but the sibling starter packages pull
`@types/react@^19`. Local hoisting happened to work; the Docker
fresh install pulled both → `'IconAlertTriangle' cannot be used as a
JSX component` etc. **Fix:** pnpm `overrides` in the workspace root
[package.json](package.json) pin react + @types/react to 18.

### 7. `DP_GITHUB_ALLOW_ORGS` shell-quoting
See [Step 2 → Shell quoting gotcha](#step-2-set-secrets). Always
single-quote the whole `KEY=["…"]` arg.

### 8. Sessions cookie missing `Secure` attribute (TODO)
`starter-auth-users` doesn't currently honour `X-Forwarded-Proto:
https` from Fly's edge, so the `starter_session` cookie is issued
without `Secure`. The browser keeps it because the origin is
already HTTPS, but strict-mode checks will flag it. Fix is in
`starter-auth-users`, not in this repo.

### 9. Entrypoint can't run subcommands while serve is up
`fly-entrypoint.sh <subcmd>` starts a fresh PG on the assumption
the container is cold-booting. If the main `serve` is already
running, `pg_ctl` fails ("another server might be running"). Use
`/usr/local/bin/dev-pulse <subcmd>` directly via `fly ssh console`
when the app is live.

---

## Common ops

```bash
APP=dev-pulse

fly logs           -a "$APP"                        # tail logs
fly ssh console    -a "$APP"                        # shell in
fly releases       -a "$APP"                        # list releases
fly deploy --image registry.fly.io/$APP:deployment-XXXX -a "$APP"   # rollback
fly scale vm shared-cpu-2x --memory 2048 -a "$APP"  # vertical scale

# Per-machine override (e.g. scheduler pinning when count > 1)
fly machine update <machine-id> --env DP_SCHEDULER_ENABLE=true -a "$APP"

# psql into the bundled PG
fly ssh console -a "$APP" -C \
  "sh -c 'su postgres -c \"psql -h /tmp devpulse\"'"

# One-off migrate (release_command equivalent)
fly ssh console -a "$APP" -C \
  '/usr/local/bin/dev-pulse migrate --config /etc/dev-pulse/config.toml'
```

---

## Troubleshooting

### "redirect_uri mismatch" from GitHub after login

`DP_PUBLIC_BASE_URL` doesn't match the GitHub OAuth App callback. Fix
either side, then redeploy/restart.

### Session cookie not sticking

1. Confirm the user hit `https://` (cookie is `Secure`).
2. Volume mount: `fly ssh console -a dev-pulse -C 'ls -la /data/auth.db'`. Empty/missing → `[mounts]` didn't apply, re-check `fly volumes list -a dev-pulse`.

### `503` immediately after deploy

Almost always a TOML parse error from a malformed secret (see #7). Check
logs: `fly logs -a dev-pulse --no-tail | tail -40`.

### Health check failing / Machine cycling

```bash
fly status -a dev-pulse
fly logs   -a dev-pulse
```

Look for:
- `Error: parse config TOML` → secret formatting bug (Step 2).
- `Address already in use` → `DP_BIND_ADDR` collides with Caddy.
- `envsubst: command not found` → runtime image missing `gettext-base`.
- `permission denied on /data/pgdata` → first-boot `chown` race; restart usually clears it.

### Webhook signatures failing

GitHub posts arrive but get `401 invalid signature`:
- Secret mismatch — rotate both at once.
- Body mangling — don't add `request_body { max_size … }` to the Caddyfile without thinking about it.

### "Cannot connect to the Docker daemon" during `fly deploy`

`make fly-deploy` already uses `--remote-only`. If you've overridden
that, start Docker locally, or just stick with the remote builder.

---

## See also

- [HTTPS-LOCAL-TEST.md](HTTPS-LOCAL-TEST.md) — local TLS pre-flight harness.
- [DOCKER.md](DOCKER.md) — plain HTTP local compose stack.
- [crates/dev-pulse/config.example.toml](crates/dev-pulse/config.example.toml) — annotated config reference.
