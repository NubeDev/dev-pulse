# Deploy dev-pulse to Fly.io

End-to-end guide for deploying the dev-pulse stack on a single Fly.io
Machine. A fresh session should be able to follow this doc top-to-bottom.

All commands run from the **repo root** (`/home/user/code/rust/dev-pulse`).
The canonical app name in this guide is **`dev-pulse`** in region
**`cdg`**, reachable at **<https://dev-pulse.fly.dev>**.

---

## What gets deployed

A single Fly Machine runs **two long-running processes** in one
container under [tini(1)](https://github.com/krallin/tini), fronted
by Fly's edge:

| # | Process | Lifetime | Port | Role |
|---|---|---|---|---|
| 1 | **Caddy 2** | long-running | 0.0.0.0:8080 | Reverse proxy — serves the built SPA from `/usr/share/dev-pulse/web` and proxies API path prefixes to the backend. |
| 2 | **dev-pulse** | long-running | 127.0.0.1:8731 | The Rust backend — axum + `dp-server`. Owns Postgres (Fly MPG), the sqlite auth sidecar at `/data/auth.db`, the GitHub OAuth + webhook surface, and the scheduler. |
| — | **dev-pulse migrate** | one-shot | — | Runs as the Fly `release_command` *before* a new release is promoted. Applies any pending `dp-store-pg` sqlx migrations atomically with the new binary that needs them. |

Fly's **fly-proxy** terminates TLS on ports 80/443 and forwards plain
HTTP to Caddy on `:8080`. Caddy is **not** the TLS terminator on Fly
(`auto_https off` in the Caddyfile); it owns static + reverse-proxy only.

### Topology

```
                          ┌──────────────────────────────────┐
  https://dev-pulse.fly.dev/  │  fly-proxy (TLS terminator)      │
  ──────────────────────►      │  shared v4 / dedicated v6        │
                          │  external 80→https + 443         │
                          └────────────┬─────────────────────┘
                                       │ http, internal :8080
                            ┌──────────▼──────────────────────┐
                            │  Caddy (path-based)             │
                            │  /auth/* /reports/* /directory/*│
                            │  /admin/* /health /openapi.json │
                            │  /metrics                       │
                            └──┬──────────────────┬───────────┘
                               │                  │
                  ┌────────────▼───────┐   ┌──────▼──────────┐
                  │  dev-pulse backend │   │ static SPA      │
                  │  127.0.0.1:8731    │   │ /usr/share/dev- │
                  │                    │   │ pulse/web       │
                  └──┬─────────────────┘   └─────────────────┘
                     │                          (try_files →
                     │                           /index.html)
        ┌────────────┴─────────────┐
        ▼                          ▼
┌─────────────────────┐  ┌──────────────────────┐
│ Fly Managed Postgres│  │ /data/auth.db        │
│ DATABASE_URL secret │  │ (Fly volume mount)   │
└─────────────────────┘  └──────────────────────┘
```

### Auth flow

1. User opens Studio → clicks **"Sign in with GitHub"**.
2. Browser → `GET /auth/oauth/github/start` (handled by `starter-auth-oauth` inside dev-pulse).
3. dev-pulse redirects to `https://github.com/login/oauth/authorize?...&redirect_uri=https://dev-pulse.fly.dev/auth/oauth/github/callback`.
4. GitHub posts back to the callback. dev-pulse exchanges the code for an access token, calls `GET /user` + `GET /user/orgs`, intersects with `[auth.github].allow_orgs`, and either mints a session (org match) or shows the awaiting-access page (no match).
5. Session cookie is written by the sqlite-backed `starter-auth-users` adapter at `/data/auth.db`. Cookie attributes: `HttpOnly; Secure; SameSite=Lax` (because the public origin is `https://`).
6. All subsequent `/auth/*`, `/reports/*`, `/directory/*`, `/admin/*` calls carry the cookie — Caddy proxies them through to `127.0.0.1:8731` and the axum auth layer validates the session.

### GitHub webhook flow

1. GitHub posts events to `https://dev-pulse.fly.dev/auth/webhook/github` (the path used by `dp-rest` — confirm against the actual handler in your repo if you wire a different prefix).
2. Caddy proxies the request to the backend, preserving the raw body so HMAC verification (`X-Hub-Signature-256`) over `[webhook].secret_ref` succeeds.
3. dev-pulse enqueues the event into Postgres and acks 200 within the GitHub 10 s deadline.

---

## Prerequisites

1. **flyctl** installed:
   ```bash
   curl -L https://fly.io/install.sh | sh
   # or: brew install flyctl
   ```
2. **Logged in**:
   ```bash
   fly auth login
   fly auth whoami
   ```
3. **Build context** = the parent directory containing both
   `dev-pulse/` and `starter/`. The Cargo + pnpm workspaces have
   path-deps into `../starter`; `make fly-deploy` cds up for you.
4. **Local HTTPS pre-flight has passed** — see [Step 0](#step-0-prove-it-works-locally-with-https) below. **Do not skip this.** The local TLS harness runs the *exact same image* behind real TLS on `https://localhost` and catches GitHub-OAuth / cookie / mixed-content bugs that only surface under HTTPS. Iterating against Fly is ~10–20 min per cycle; iterating locally is ~30 s.

---

## Step 0: Prove it works locally with HTTPS

See [HTTPS-LOCAL-TEST.md](HTTPS-LOCAL-TEST.md) for the full recipe. TL;DR:

```bash
# One-time
sudo apt install -y mkcert libnss3-tools
mkcert -install

# Bring up the same image we're about to deploy to Fly
make fly-local

# Open https://localhost in a browser, sign in with GitHub.
# Tear down (keeps the volume):
make fly-local-down
```

If anything is red on `https://localhost`, **stop** — fix it locally
before burning a Fly deploy.

---

## Step 1: Install fly CLI (if missing)

```bash
if ! command -v fly &>/dev/null; then
    curl -L https://fly.io/install.sh | sh
    export FLYCTL_INSTALL="$HOME/.fly"
    export PATH="$FLYCTL_INSTALL/bin:$PATH"
fi
fly version
fly auth login
fly auth whoami
```

---

## Step 2: Create app + volume + Postgres (first time only)

```bash
APP=dev-pulse
REGION=cdg

# 2a. Reserve the app name (does NOT deploy).
fly apps create "$APP"

# 2b. Persistent volume for the sqlite auth sidecar (sessions, tokens,
#     oauth identities). Mounted at /data by fly.toml.
fly volumes create dev_pulse_data --region "$REGION" --size 1 -a "$APP" --yes

# 2c. Managed Postgres for dp-data. `attach` sets DATABASE_URL on the
#     app as a Fly secret automatically.
fly mpg create  --name "$APP-db" --region "$REGION" --plan basic
fly mpg attach  "$APP-db" --app "$APP"

# Verify the secret was set:
fly secrets list -a "$APP" | grep DATABASE_URL
```

> **Note.** If you'd rather use external Postgres (Supabase / Neon /
> self-hosted), skip 2c and `fly secrets set DATABASE_URL=...` instead.
> The connection URL must be reachable from the Fly Machine's egress
> IP and (for managed providers outside Fly) use TLS — append
> `?sslmode=require` if your driver doesn't infer it.

---

## Step 3: Set application secrets

```bash
APP=dev-pulse

fly secrets set -a "$APP" \
  GITHUB_WEBHOOK_SECRET="$(openssl rand -hex 32)" \
  OAUTH_GITHUB_CLIENT_SECRET="<github-oauth-app-client-secret>" \
  GITHUB_PAT="ghp_<your-pat-or-app-installation-token>" \
  DP_GITHUB_OAUTH_CLIENT_ID="Iv1.deadbeef..." \
  DP_GITHUB_ALLOW_ORGS='["NubeIO"]' \
  DP_PUBLIC_BASE_URL="https://$APP.fly.dev"

# Verify:
fly secrets list -a "$APP"
```

| Secret | Source | Notes |
|---|---|---|
| `DATABASE_URL` | `fly mpg attach` (Step 2c) | Postgres connection string. Resolved by `config.fly.toml`. |
| `GITHUB_WEBHOOK_SECRET` | random 32-byte hex | Must match what you register in the GitHub App / repo webhook settings. Resolved via `secret://github/webhook_secret`. |
| `OAUTH_GITHUB_CLIENT_SECRET` | GitHub OAuth App | Resolved via `secret://oauth/github_client_secret`. |
| `GITHUB_PAT` | Classic / fine-grained PAT *or* GitHub App installation token | The fetcher's read token. Resolved via `secret://github/pat`. Leave unset to keep the fetcher dormant. |
| `DP_GITHUB_OAUTH_CLIENT_ID` | GitHub OAuth App | Non-secret but env-driven so the same image works for multiple orgs. |
| `DP_GITHUB_ALLOW_ORGS` | operator | JSON array of GitHub org logins allowed past the org gate. **Must be valid JSON** — `envsubst` writes it verbatim into the TOML. |
| `DP_PUBLIC_BASE_URL` | operator | The public origin. Drives the OAuth `redirect_uri` and every absolute URL the backend emits. Mismatch with what's registered on the GitHub OAuth App → 401 on callback. |

### GitHub OAuth App configuration

In the GitHub OAuth App settings, set:

- **Homepage URL** — `https://dev-pulse.fly.dev`
- **Authorization callback URL** — `https://dev-pulse.fly.dev/auth/oauth/github/callback`

For local TLS pre-flight, register a **separate** OAuth App with
callback `https://localhost/auth/oauth/github/callback` (see
[HTTPS-LOCAL-TEST.md](HTTPS-LOCAL-TEST.md)).

---

## Step 4: Allocate IPs (first time only)

Fly allocates a shared IPv4 by default for `[http_service]` apps —
this step is needed only if you want a **dedicated** v4 (for example
to use a custom domain with strict CAA records). Dedicated v4 has a
monthly cost; shared is free.

```bash
fly ips list -a dev-pulse

# Optional:
fly ips allocate-v4         -a dev-pulse   # dedicated v4 ($)
fly ips allocate-v6         -a dev-pulse   # dedicated v6 (free)
```

---

## Step 5: Deploy

```bash
make fly-deploy
```

Under the hood that runs (from the parent of this repo so the workspace
path-deps resolve):

```bash
fly deploy \
    --app dev-pulse \
    --config dev-pulse/fly.toml \
    --dockerfile dev-pulse/Dockerfile.fly \
    --remote-only \
    .
```

**First build: ~10–20 min** (full Rust release compile + pnpm SPA build).
Subsequent builds with Fly's remote cache + the cargo target cache mount
inside the Dockerfile: ~3–6 min.

The release command (`dev-pulse migrate ...`) runs **before** the new
Machine accepts traffic, so a migration that fails aborts the deploy
with the old release still serving requests.

---

## Step 6: Verify

```bash
APP=dev-pulse

# Machines / VM state
fly status -a "$APP"
fly machines list -a "$APP"

# Health (proxied through Caddy → backend)
curl -sI "https://$APP.fly.dev/health" | head -1

# OpenAPI doc (proves backend wiring)
curl -s  "https://$APP.fly.dev/openapi.json" | python3 -m json.tool | head -20

# SPA index served by Caddy
curl -sI "https://$APP.fly.dev/" | head -3
curl -s  "https://$APP.fly.dev/" | grep -oE '<title>[^<]+</title>'

# Logs (Ctrl-C to detach)
make fly-logs

# Shell into the running Machine
make fly-ssh
```

### Expected processes inside the container

```
$ make fly-ssh -- ps -eo pid,comm
  PID COMMAND
    1 tini
    7 fly-entrypoint
    9 caddy
   17 dev-pulse
```

(The exact PIDs differ; the four binaries are what matter.)

---

## Step 7: First login

1. Open **<https://dev-pulse.fly.dev/>** in a browser.
2. Click **"Sign in with GitHub"**.
3. Authorize the OAuth App for one of the orgs in
   `DP_GITHUB_ALLOW_ORGS`.
4. You should land on the home view with a session cookie set.

If you land on the awaiting-access page instead, your GitHub user
isn't a member of any allow-listed org (or the org membership is
**private** and you haven't approved the OAuth App to read it — check
GitHub → Settings → Applications → Authorized OAuth Apps → grant the
org).

### Bootstrap a local admin (optional)

If you want to avoid the OAuth dance for first-time setup:

```bash
fly ssh console -a dev-pulse -C \
  '/usr/local/bin/dev-pulse create-admin \
     --config /etc/dev-pulse/config.toml \
     --email "you@example.com" \
     --password "$(openssl rand -base64 18)"'
```

This writes a local password-auth row into `/data/auth.db` so you can
sign in without GitHub. **Do not** ship a known password to production.

---

## Custom domain

```bash
APP=dev-pulse
DOMAIN=dev-pulse.example.com

fly certs add "$DOMAIN" -a "$APP"
# Follow the DNS instructions flyctl prints (A + AAAA, or CNAME).
fly certs show "$DOMAIN" -a "$APP"

# Once the cert is issued, point the backend at the new origin:
fly secrets set DP_PUBLIC_BASE_URL="https://$DOMAIN" -a "$APP"
# Update the GitHub OAuth App callback to https://$DOMAIN/auth/oauth/github/callback
# Then re-deploy so the new config.toml is materialised:
make fly-deploy
```

---

## Key files

| File | Purpose |
|---|---|
| [fly.toml](fly.toml) | Fly app config — region, VM size, env, `[http_service]`, `[mounts]`, `release_command`. |
| [Dockerfile.fly](Dockerfile.fly) | Multi-stage build: backend (Rust) → SPA (pnpm) → runtime (debian-slim + Caddy + tini + gettext-base). |
| [Caddyfile](Caddyfile) | Reverse-proxy table + SPA fallback. Mirrors [frontend/nginx.conf](frontend/nginx.conf) so local-compose and Fly behave identically. |
| [scripts/fly-entrypoint.sh](scripts/fly-entrypoint.sh) | Container init — `envsubst`s `config.template.toml` → `config.toml`, then runs Caddy + dev-pulse (or execs `dev-pulse <subcmd>` for the release command). |
| [crates/dev-pulse/config.fly.toml](crates/dev-pulse/config.fly.toml) | Runtime config **template** with `${VAR}` placeholders. Materialised at boot. |
| [docker-compose.fly-local.yml](docker-compose.fly-local.yml) | Local pre-flight harness — Caddy(TLS) + the canonical Fly image + Postgres. See [HTTPS-LOCAL-TEST.md](HTTPS-LOCAL-TEST.md). |
| [Caddyfile.local](Caddyfile.local) | Local-TLS Caddy: `localhost { tls /certs/localhost.pem … reverse_proxy app:8080 }`. |
| [Makefile](Makefile) | `fly-deploy`, `fly-logs`, `fly-ssh`, `fly-status`, `fly-secrets-print`, `fly-local`, `fly-local-down`, `fly-local-reset`. |

---

## Environment variables

Set in [fly.toml](fly.toml) (non-secret) or via `fly secrets set` (secret).
All are consumed by [scripts/fly-entrypoint.sh](scripts/fly-entrypoint.sh)
and substituted into [crates/dev-pulse/config.fly.toml](crates/dev-pulse/config.fly.toml)
via `envsubst`.

| Variable | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | **required** (set by `fly mpg attach`) | Postgres URL for `dp-data`. |
| `DP_PUBLIC_BASE_URL` | `https://dev-pulse.fly.dev` (fly.toml) | Public origin. Drives the OAuth `redirect_uri` and every absolute URL emitted by the backend. |
| `DP_BIND_ADDR` | `127.0.0.1:8731` | Where the Rust backend listens **inside the container**. Caddy proxies to this. |
| `DP_DEFAULT_RETURN` | `/` | Where `/auth/oauth/github/start` returns users when no `return_to` is set. |
| `DP_AUTH_SQLITE_URL` | `sqlite:/data/auth.db?mode=rwc` | Path on the Fly volume for the auth sidecar (sessions, tokens, oauth identities). |
| `DP_SCHEDULER_ENABLE` | `false` | When `true` *and* a `GITHUB_PAT` secret is set, the reconciler runs every `[scheduler].tick_interval_secs`. Keep `false` on Machines > 1 unless you pin the scheduler to one Machine via per-machine env. |
| `DP_GITHUB_OAUTH_CLIENT_ID` | (Fly secret) | GitHub OAuth App client ID. |
| `DP_GITHUB_ALLOW_ORGS` | (Fly secret) | JSON array of allowed GitHub org logins. **Must be valid JSON**. |
| `GITHUB_PAT` | (Fly secret) | Resolves `secret://github/pat`. Leave unset to keep the fetcher dormant. |
| `GITHUB_WEBHOOK_SECRET` | (Fly secret) | Resolves `secret://github/webhook_secret`. |
| `OAUTH_GITHUB_CLIENT_SECRET` | (Fly secret) | Resolves `secret://oauth/github_client_secret`. |
| `RUST_LOG` | `info,dev_pulse=info,dp_server=info,sqlx=warn` | Tracing filter. |

### The reuse principle: one image, two targets

The **Fly image is the canonical artifact**. The local-TLS harness
([docker-compose.fly-local.yml](docker-compose.fly-local.yml)) and the
Fly deployment ([fly.toml](fly.toml)) both run the **same
`dev-pulse-fly:local` image** built from
[Dockerfile.fly](Dockerfile.fly). They differ only in the env vars
they inject and in what's in front of the container (Fly's edge vs. a
local Caddy with `mkcert` certs).

Conventions:

- Anything that should differ between prod and local-TLS is a
  `DP_*` env var, defaulted in `fly-entrypoint.sh`, overridden in
  [fly.toml](fly.toml) (production) or [docker-compose.fly-local.yml](docker-compose.fly-local.yml) (local-TLS).
- **Never** introduce a second Dockerfile, a parallel entrypoint, or a
  "local-only" code path. If you find yourself wanting to, add an
  env-var knob instead.

---

## Persistent volume layout (`/data`)

```
/data/
└── auth.db          # sqlite — starter-auth-users + starter-auth-oauth row families
                     # (users, sessions, tokens, oauth_identities).
                     # Schemas applied automatically at boot.
```

Postgres state (events, memberships, fetch_runs, audit_log) lives in
**Fly Managed Postgres**, not on this volume.

### Wipe recipes

- **Reset sessions / OAuth identities only** (real users keep their
  Postgres rows; everyone is just signed out):
  ```bash
  fly ssh console -a dev-pulse -C 'rm -f /data/auth.db && /usr/local/bin/dev-pulse migrate --config /etc/dev-pulse/config.toml'
  ```
- **Reset dp-data (events, memberships, audit log)** — drops the
  managed Postgres DB and re-runs migrations:
  ```bash
  fly mpg destroy dev-pulse-db    # ← irreversible
  fly mpg create  --name dev-pulse-db --region cdg --plan basic
  fly mpg attach  dev-pulse-db --app dev-pulse
  make fly-deploy                  # release_command re-runs migrate
  ```

---

## Common ops

```bash
APP=dev-pulse

# Tail logs (Ctrl-C to detach)
fly logs -a "$APP"

# Shell into the running Machine
fly ssh console -a "$APP"

# Run a one-off migrate without a release:
fly ssh console -a "$APP" -C \
  '/usr/local/bin/dev-pulse migrate --config /etc/dev-pulse/config.toml'

# List recent releases / rollback
fly releases -a "$APP"
fly deploy   --image registry.fly.io/$APP:deployment-XXXX -a "$APP"

# Scale up vertically
fly scale vm shared-cpu-2x --memory 2048 -a "$APP"

# Add a second Machine (horizontal). REMEMBER: keep scheduler on ONLY ONE.
fly machines list -a "$APP"
fly scale count 2 -a "$APP"
fly machine update <machine-id> --env DP_SCHEDULER_ENABLE=true -a "$APP"
fly machine update <other-id>   --env DP_SCHEDULER_ENABLE=false -a "$APP"
```

---

## Troubleshooting

### "redirect_uri mismatch" from GitHub after login

The `DP_PUBLIC_BASE_URL` secret doesn't match what's registered on the
GitHub OAuth App. Fix:

```bash
fly secrets list -a dev-pulse | grep DP_PUBLIC_BASE_URL
# Compare with GitHub OAuth App → Authorization callback URL.
# Either update the OAuth App, or:
fly secrets set DP_PUBLIC_BASE_URL=https://dev-pulse.fly.dev -a dev-pulse
make fly-deploy
```

### Session cookie not sticking

Symptoms: sign-in succeeds, the next page is logged out again.

Likely causes:

1. **Mixed `http://` / `https://` access** — the cookie is `Secure`,
   the browser drops it over HTTP. Confirm `DP_PUBLIC_BASE_URL` is
   `https://...` *and* the user is hitting the same scheme.
2. **`/data` volume not mounted** — sqlite re-creates a fresh DB on
   tmpfs each boot, sessions don't survive a restart. Check:
   ```bash
   fly ssh console -a dev-pulse -C 'ls -la /data/auth.db'
   ```
   Should show a non-zero file. If it's missing or 0 bytes, the
   `[mounts]` block in `fly.toml` didn't take effect — re-run
   `fly volumes list -a dev-pulse` and re-deploy.

### "DATABASE_URL is required" on boot

`fly mpg attach` wasn't run, or it ran against a different app.

```bash
fly secrets list -a dev-pulse | grep DATABASE_URL
fly mpg list
fly mpg attach dev-pulse-db --app dev-pulse
```

### Health check failing / Machine cycling

```bash
fly status -a dev-pulse
fly logs   -a dev-pulse
```

Look for:

- `pool timed out while waiting for an open connection` — Postgres is
  unreachable, or the connection limit is exhausted.
  `fly mpg status dev-pulse-db`.
- `Address already in use` — `DP_BIND_ADDR` collides with Caddy. Both
  must differ; Caddy is `:8080`, backend is `127.0.0.1:8731`.
- `envsubst: command not found` — the runtime image is missing
  `gettext-base`. Confirm `Dockerfile.fly` still installs it.

### Webhook signatures failing

GitHub posts arrive but get `401 invalid signature`:

- The webhook secret registered on GitHub doesn't match
  `GITHUB_WEBHOOK_SECRET`. Rotate both at once.
- Caddy is mangling the body. Caddy 2 streams the body untouched for
  `reverse_proxy`, but if you added `request_body { max_size ... }`
  with a small limit, large pushes get truncated → signature fails.

### "Cannot connect to the Docker daemon" during `fly deploy`

The default build is remote (the `--remote-only` flag in
`make fly-deploy`). If you've overridden that and the local Docker
daemon isn't running:

```bash
sudo systemctl start docker     # or open Docker Desktop
# or just use the remote builder:
make fly-deploy
```

---

## See also

- [HTTPS-LOCAL-TEST.md](HTTPS-LOCAL-TEST.md) — local TLS pre-flight harness.
- [DOCKER.md](DOCKER.md) — plain HTTP local compose stack (no TLS, dev only).
- [crates/dev-pulse/config.example.toml](crates/dev-pulse/config.example.toml) — annotated config reference.
