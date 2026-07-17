# Deploy dev-pulse to Coolify

End-to-end runbook for deploying dev-pulse to [Coolify](https://coolify.io/docs/),
alongside (not replacing) the [Fly.io deploy](FLY.md). See
[COOLIFY-SCOPE.md](COOLIFY-SCOPE.md) for the design decisions behind
this — read that first if you're wondering *why* something is shaped
the way it is.

All commands run from `/home/user/code/rust/dev-pulse` unless noted.

---

## What gets deployed

One Coolify **application container** runs two processes under
[tini(1)](https://github.com/krallin/tini):

| # | Process | Port | Role |
|---|---|---|---|
| 1 | **Caddy 2** | 0.0.0.0:8080 | Static SPA + reverse proxy. |
| 2 | **dev-pulse** | 127.0.0.1:8731 | Rust backend (axum + dp-server). Owns the sqlite auth sidecar at `/data/auth.db`, OAuth + webhook surfaces, and the scheduler. |

Postgres is **not** bundled in the image. It's a separate **Coolify
Postgres resource** on the same server — first-class UI, S3 backups,
no `pg_ctl` fighting the entrypoint. This is the one structural
difference from the Fly deploy (which bundles PG on the volume
because Fly Managed Postgres isn't available in `cdg`). See
COOLIFY-SCOPE.md §2.3.

Coolify's **Traefik** terminates TLS at the edge (auto Let's Encrypt)
and forwards plain HTTP to the container on `:8080` — the same role
fly-proxy plays for the Fly deploy. `auto_https off` in the
[Caddyfile](Caddyfile) stays correct unchanged.

### Topology

```
                    https://<your-domain>/
                              │
                    ┌─────────▼──────────────┐
                    │ Traefik (TLS, Coolify) │
                    └─────────┬──────────────┘
                              │ plain HTTP, internal :8080
                    ┌─────────▼──────────────┐
                    │ Caddy                  │
                    │ - serves the SPA       │
                    │ - reverse_proxy →      │
                    │   127.0.0.1:8731       │
                    └────────┬───────────────┘
                             │
                  ┌──────────▼──────────────┐
                  │ dev-pulse backend       │
                  │ 127.0.0.1:8731          │
                  └─┬───────────────────┬───┘
                    │                   │
       ┌────────────▼─────────┐  ┌──────▼──────────────┐
       │ Coolify Postgres      │  │ /data/auth.db       │
       │ resource (managed,    │  │ (sqlite — sessions, │
       │ separate container)   │  │  oauth identities)  │
       └───────────────────────┘  └──────────┬───────────┘
                                              │
                                   Coolify persistent volume
```

---

## Prerequisites

1. **`starter`'s dependency surface must be fully pushed to
   `origin/master`.** Unlike Fly (which builds from your local working
   directory, uncommitted changes included), the `starter` submodule
   only sees what's actually on GitHub. If `dev-pulse`'s `main` depends
   on `starter` code that only exists locally and uncommitted, the
   Coolify build fails on a missing symbol even though `fly deploy`
   works fine. Concretely, as of this writing `dev-pulse`'s
   `crates/dp-rest/src/me_password.rs` calls
   `starter_auth_users::admin::change_password`, which exists only in
   the local `rust/starter` working tree's uncommitted diff — **the
   Coolify build will fail until that lands on `starter`'s `master`.**
   Run `make coolify-local-build` after any `starter` change that
   `dev-pulse` newly depends on, to catch this before it hits Coolify.
2. A Coolify server, reachable and provisioned. **Rust release
   builds take ~10–15 min and several GB of RAM**, and Coolify builds
   on the target server itself (no remote builder farm) — make sure
   the box isn't a small shared VPS, or the build will peg it
   alongside the running app. If it's undersized, build the image in
   CI instead and point Coolify at a pre-built image (Coolify's
   "Docker Image" build pack) — see COOLIFY-SCOPE.md §2.1 option (c).
2. `git submodule update --init` in this repo, so `./starter` is
   populated locally (only needed for local image testing —
   Coolify's own clone checks out submodules on its own).
3. A **second GitHub OAuth App** if you're running this alongside
   Fly — the two deploys can't share one callback URL (see
   [GitHub OAuth App config](#github-oauth-app-config) below).

---

## Step 0: Prove the image builds and runs locally

```bash
make coolify-local
# ...
make coolify-local-logs
make coolify-local-down     # keeps volumes
```

This builds `Dockerfile.coolify` with build context = this repo (not
the parent dir — see COOLIFY-SCOPE.md §2.1) and runs it against a
local Postgres container standing in for the Coolify Postgres
resource. Catches build/entrypoint regressions before burning a
Coolify build cycle.

---

## Step 1: Create the Coolify application

In the Coolify UI:

1. **New Resource → Application → your Git source → this repo.**
2. **Build Pack**: Dockerfile.
3. **Base Directory**: `/` (this repo *is* the deploy unit — no
   parent-directory trick needed, unlike Fly. `starter` resolves via
   the git submodule Coolify checks out automatically).
4. **Dockerfile Location**: `Dockerfile.coolify`.
5. **Ports Exposes**: `8080`.
6. **Health Check Path**: `/health`.
7. **Domain**: `https://<your-domain>` — Traefik issues the cert.
8. **Persistent Storage**: volume → `/data` (holds only `auth.db`,
   small — no Postgres data lives here, unlike Fly).

---

## Step 2: Create the Postgres resource

**New Resource → Database → PostgreSQL** (version 16, to match). Once
it's up, copy its **internal connection string** — Coolify shows this
in the resource's "Internal" connection tab (something like
`postgres://<user>:<pass>@<resource-name>:5432/<db>`, reachable from
other containers on the same Coolify network, not from the internet).

---

## Step 3: Configure environment variables

In the application's **Environment Variables** tab:

| Variable | Value | Notes |
|---|---|---|
| `DATABASE_URL` | the Postgres resource's internal URL from Step 2 | **Runtime + no default** — the entrypoint fails fast if unset (COOLIFY-SCOPE.md §2.3 drops the Fly-style auto-build-from-`POSTGRES_PASSWORD` fallback, since there's no bundled PG to build a URL for). |
| `DP_PUBLIC_BASE_URL` | `https://<your-domain>` | Drives the OAuth `redirect_uri` and every absolute URL the backend emits. |
| `DP_BIND_ADDR` | `127.0.0.1:8731` | Unchanged from Fly. |
| `DP_DEFAULT_RETURN` | `/` | Unchanged from Fly. |
| `DP_AUTH_SQLITE_URL` | `sqlite:/data/auth.db?mode=rwc` | Unchanged from Fly. |
| `DP_SCHEDULER_ENABLE` | `false` initially | Flip once seeded — see Step 6. |
| `DP_GITHUB_OAUTH_CLIENT_ID` | GitHub OAuth App client ID | Not secret, but env-driven. |
| `RUST_LOG` | `info,dev_pulse=info,dp_server=info,sqlx=warn` | Unchanged from Fly. |

Mark these as **runtime secrets** (Coolify's secret toggle on the env
var):

| Secret | Source | Notes |
|---|---|---|
| `GITHUB_PAT` | classic / fine-grained PAT | Same scopes as Fly: `repo:status, public_repo, read:org, read:user`. |
| `GITHUB_WEBHOOK_SECRET` | `openssl rand -hex 32` | Paste the same value into the GitHub App webhook settings. |
| `OAUTH_GITHUB_CLIENT_SECRET` | GitHub OAuth App | Client secret. |
| `DP_GITHUB_ALLOW_ORGS` | operator, e.g. `["NubeIO","NubeDev","PJNube"]` | JSON array, pasted into a Coolify UI field — no shell in the path, so the FLY.md pitfall #7 quoting trap doesn't apply here. It's still `envsubst`'d raw into TOML, so a malformed value still crash-loops the container — double-check it's valid JSON before saving. |

`POSTGRES_PASSWORD` **drops out entirely** — there's no bundled PG
role for it to configure.

### GitHub OAuth App config

- **Homepage URL** — `https://<your-domain>`
- **Authorization callback URL** — `https://<your-domain>/auth/oauth/github/callback`

If Fly is still live, this **must** be a separate OAuth App from the
one Fly uses — they can't share a callback URL.

---

## Step 4: Deploy

Push to the branch Coolify is watching (or hit **Deploy** in the UI —
Coolify builds `Dockerfile.coolify` on the server, no CLI step here).

Migrations run **inside the entrypoint** before Caddy and dev-pulse
start, same placement as Fly — Coolify has no `release_command`
either.

First build: ~10–15 min (Rust release + pnpm SPA), same as Fly. No
Postgres apt-install step to slow it down further, which is the one
place this is faster than Fly's image.

---

## Step 5: Seed the admin user

Use Coolify's browser terminal, or `docker exec` on the host if you
have shell access to the server:

```bash
docker exec -it <container-id> sh -c \
  "cd /app && /usr/local/bin/dev-pulse create-admin \
    --config /etc/dev-pulse/config.toml \
    --email dev@dev.com \
    --password dev123456789"
```

Unlike Fly (FLY.md pitfall #9), there's no bundled PG for a second
invocation to race with — `coolify-entrypoint.sh`'s non-`serve`
branch execs `dev-pulse <subcmd>` directly, nothing else to start.

Verify:

```bash
curl -i -X POST https://<your-domain>/auth/login \
  -H 'content-type: application/json' \
  -d '{"email":"dev@dev.com","password":"dev123456789"}'
# Expect: HTTP/2 200 with `set-cookie: starter_session=…`
```

---

## Step 6: Seed orgs + repos, enable the scheduler

Same commands as Fly (FLY.md Step 5), just shelled in differently:

```bash
docker exec -it <container-id> sh -c \
  "cd /app && /usr/local/bin/dev-pulse import-my-orgs \
    --config /etc/dev-pulse/config.toml"

docker exec -it <container-id> sh -c \
  "cd /app && /usr/local/bin/dev-pulse import-my-repos \
    --config /etc/dev-pulse/config.toml \
    --orgs NubeDev,NubeIO,PJNube \
    --active-within-days 365 \
    --max 200"
```

Trigger the first sync and enable the scheduler exactly as in
[FLY.md → Trigger the first sync](FLY.md#trigger-the-first-sync) /
[→ Enable the scheduler](FLY.md#enable-the-scheduler-recommended) —
set `DP_SCHEDULER_ENABLE=true` in the Coolify Environment Variables
tab and redeploy (or restart, if Coolify applies env changes without
a rebuild — check the resource's env var docs).

---

## Step 7: Verify

```bash
curl -sI  "https://<your-domain>/health"
curl -s   "https://<your-domain>/openapi.json" | python3 -m json.tool | head
curl -sI  "https://<your-domain>/" | head -3
```

Check the Coolify UI's health-check indicator (gates Traefik
routing — an unhealthy container gets no traffic even if it's up).

---

## Key files

| File | Purpose |
|---|---|
| [Dockerfile.coolify](Dockerfile.coolify) | Multi-stage: Rust release → pnpm SPA → debian-slim runtime with Caddy + tini + gettext-base + curl. **No Postgres install.** Build context is this repo; recreates the `/src/starter` + `/src/dev-pulse` sibling layout Cargo/pnpm expect by relocating the `starter` submodule inside the build stages. |
| [.dockerignore](.dockerignore) | Scoped to this repo as build context (not the parent dir — that's `../.dockerignore`, used by Fly/compose). |
| [Caddyfile](Caddyfile) | Unchanged from Fly — Traefik plays fly-proxy's role identically. |
| [scripts/coolify-entrypoint.sh](scripts/coolify-entrypoint.sh) | Fork of `fly-entrypoint.sh` with `start_postgres`/`stop_postgres` removed. Still does envsubst → `/etc/dev-pulse/config.toml`, still runs `migrate` before serve, still `cd /app` for the authz policy path, still Caddy-background + backend-foreground under tini. Fails fast if `DATABASE_URL` isn't set. |
| [crates/dev-pulse/config.coolify.toml](crates/dev-pulse/config.coolify.toml) | Runtime config template, identical shape to `config.fly.toml`. |
| [docker-compose.coolify-local.yml](docker-compose.coolify-local.yml) | Local smoke-test harness — app + a throwaway Postgres container standing in for the Coolify resource. |
| [.gitmodules](.gitmodules) | Pins the `starter` submodule at `./starter` (COOLIFY-SCOPE.md §2.1 option (a)). |
| [Makefile](Makefile) | `coolify-local`, `coolify-local-down`, `coolify-local-reset`, `coolify-local-logs`, `coolify-submodule-update`. No `coolify-deploy` — Coolify deploys via git-push or its API, not a CLI wrapper. |

---

## Environment variables

See the [Step 3](#step-3-configure-environment-variables) table above
for the full list. Everything not called out there as changed is
identical to [FLY.md → Environment variables](FLY.md#environment-variables).

---

## Persistent volume layout (`/data`)

```
/data/
└── auth.db         # sqlite — starter-auth-users + starter-auth-oauth row families
```

Much smaller than Fly's volume — no `pgdata/` here, Postgres data
lives in the Coolify Postgres resource's own volume, backed up
through Coolify's built-in S3 backup UI (configure that on the
Postgres resource, not the app).

### Wipe recipes

- **Reset sessions only** (PG data untouched, it's a separate
  resource):
  ```bash
  docker exec -it <container-id> rm -f /data/auth.db
  # restart the app container from the Coolify UI
  ```
- **Full reset** — wipe the Postgres resource from its own Coolify UI
  (separate from the app), and/or delete + recreate the `/data`
  volume from the app's Storage tab.

---

## Updating the `starter` submodule

`starter` is vendored as a git submodule, not a sibling checkout —
bumping it is a deliberate step, not automatic:

```bash
make coolify-submodule-update   # pulls latest starter, updates the pointer
git add starter
git commit -m "bump starter submodule"
git push
```

Coolify then rebuilds against the new pointer on the next deploy.

---

## What carries over unchanged from Fly

Worth stating explicitly — most of the risk surface, none of it
moves. See FLY.md's [Operational pitfalls](FLY.md#operational-pitfalls-we-hit-and-fixed)
for the war stories behind each of these:

- **The Caddyfile** — Traefik plays fly-proxy's exact role. `auto_https
  off` stays correct. The `handle`-block ordering fix (FLY.md
  pitfall #4) stays correct.
- **Migrations in the entrypoint** — Coolify has no `release_command`
  either.
- **The authz policy relative-path workaround** (FLY.md pitfall #3).
- **The pnpm react/@types/react overrides** (FLY.md pitfall #6).
- **Rust MSRV pinning** (FLY.md pitfall #2) — `Dockerfile.coolify`
  uses the same `rust:1.90-slim-bookworm` base, bump in lockstep.

## What's different from Fly

- **No bundled Postgres** (§2.3) — a Coolify resource instead. This
  also means the `*)` subcommand branch in the entrypoint no longer
  races a second PG instance (FLY.md pitfall #9 doesn't exist here).
- **Build context is this repo**, not the parent directory — no
  `--ignorefile` flag exists on Coolify, so `.dockerignore` in the
  repo root is authoritative, and `starter` is a submodule rather
  than a sibling checkout.
- **`DP_GITHUB_ALLOW_ORGS`** is pasted into a UI field, not a shell
  arg — the quoting trap in FLY.md pitfall #7 is specific to
  `fly secrets set`'s shell invocation and doesn't apply here (the
  JSON-validity requirement still does).

---

## Troubleshooting

Symptoms and fixes largely mirror [FLY.md → Troubleshooting](FLY.md#troubleshooting).
The Coolify-specific deltas:

### Container crash-loops immediately with a TOML parse error

Same root cause as FLY.md's `503` entry — malformed
`DP_GITHUB_ALLOW_ORGS` JSON. Check the container logs in the Coolify
UI (or `docker logs <container-id>`).

### `DATABASE_URL must be set to the Coolify Postgres resource's connection string`

The entrypoint's fail-fast guard tripped — `DATABASE_URL` is unset or
empty in the Environment Variables tab. Copy it fresh from the
Postgres resource's Internal connection tab (Step 2); it can change
if the resource is recreated.

### Health check failing / container restarting

Check container logs for the same signatures as FLY.md's
[Health check failing](FLY.md#health-check-failing--machine-cycling)
section (`envsubst: command not found`, TOML parse errors). The
`permission denied on /data/pgdata` failure mode in that list is
Fly-specific and can't happen here — there's no `/data/pgdata`.

### Build times out or pegs the server

See [Prerequisites](#prerequisites) #1 — the Coolify server builds on
itself, not a remote builder. If this bites, move to a CI-built image
(COOLIFY-SCOPE.md §2.1 option (c)) and switch the app's build pack
from Dockerfile to Docker Image.

---

## See also

- [COOLIFY-SCOPE.md](COOLIFY-SCOPE.md) — the design doc this runbook implements.
- [FLY.md](FLY.md) — the Fly.io deploy this one runs alongside.
- [DOCKER.md](DOCKER.md) — plain HTTP local compose stack (unrelated to either deploy target).
