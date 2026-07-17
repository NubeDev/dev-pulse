# Scope: Deploy dev-pulse on Coolify

Status: **implemented**. All deliverables in §3 landed; runbook is
[COOLIFY.md](COOLIFY.md). This document is kept as the design record —
the "why" behind the choices below. Decisions taken (§7 answered):

- **§2.1** — option **(a)**, submodule: `starter` is vendored at
  `dev-pulse/starter` via `.gitmodules`, pinned to the same commit as
  the local sibling checkout at implementation time. `Dockerfile.coolify`
  recreates the `/src/starter` + `/src/dev-pulse` sibling layout inside
  the build stages by relocating the submodule dir — so none of the
  `../starter/...` path references in `Cargo.toml` / `pnpm-workspace.yaml`
  needed to change, and local (non-Docker) dev is untouched.
- **§2.4 naming** — **forked**, not generalised, despite the
  recommendation below. Fly and Coolify Dockerfiles have genuinely
  different build contexts (parent dir vs. this repo) and the entrypoints
  diverge structurally (bundled-PG lifecycle vs. none) — a shared file
  would need conditionals for both, which is worse than two small files.
  `Dockerfile.fly` / `fly-entrypoint.sh` / `config.fly.toml` are
  untouched; `Dockerfile.coolify` / `scripts/coolify-entrypoint.sh` /
  `config.coolify.toml` are new, parallel files.
- **Fly stays alive in parallel** — this is additive, not a migration.
  Running both means two GitHub OAuth Apps (they can't share a callback
  URL) — see COOLIFY.md's OAuth App section.
- **§4** — Dockerfile build pack + managed Coolify Postgres resource,
  per the recommendation below. No `docker-compose.coolify.yml` for
  production; a `docker-compose.coolify-local.yml` exists only for
  local image smoke-testing (`make coolify-local`).
- **Server sizing (open question #2)** — not verified against a real
  Coolify server as part of this change; COOLIFY.md's Prerequisites
  section flags it and names the CI-built-image fallback if the build
  turns out to peg the box.

---

## 1. Why this is mostly a re-wiring job, not a rewrite

The app is already containerised and already runs behind a reverse
proxy that terminates nothing. Coolify's model is very close to Fly's:

| Concern | Fly today | Coolify equivalent |
|---|---|---|
| TLS termination | fly-proxy at the edge | Traefik at the edge, auto Let's Encrypt |
| App receives | plain HTTP on an internal port | plain HTTP on an internal port |
| Non-secret env | `[env]` in `fly.toml` | Environment Variables tab |
| Secrets | `fly secrets set` | same tab, marked as build-time/runtime |
| Persistent disk | `[mounts]` → `/data` | Persistent Storage (volume or bind mount) |
| Health check | `[http_service.checks]` | per-app health check, gates Traefik routing |
| Build | `Dockerfile.fly`, parent context | Dockerfile build pack, **Base Directory** setting |

So the single-image `Dockerfile.fly` (Caddy + Postgres + backend under
tini, listening on `:8080`) drops into Coolify's **Dockerfile build
pack** almost unchanged. The work is in the four places where we hard-
coded Fly assumptions.

---

## 2. The four real problems

### 2.1 The build context is the parent directory

This is the biggest one. `Dockerfile.fly` does `COPY starter` and
`COPY dev-pulse` because the Cargo path-deps and the pnpm workspace
reference a sibling `../starter` checkout. On Fly we solve it by
running `fly deploy` from `/home/user/code/rust` with an explicit
`--ignorefile`.

Coolify builds from **one git repository clone**. It gives you a *Base
Directory* (where to build from) and a *Dockerfile Location*, but it
cannot clone two sibling repos into a shared parent. Options, in the
order I'd try them:

- **(a) Vendor `starter` as a git submodule or subtree** under
  `dev-pulse/`. Coolify does check out submodules. Lowest-risk, keeps
  one repo per app, but you now have a submodule pointer to bump.
- **(b) Publish `starter` crates + packages to a registry** (crates.io
  / a private registry / GitHub Packages) and depend on versions
  instead of paths. Cleanest long-term, largest amount of work, and it
  slows down the tight local edit loop you currently have across the
  two trees.
- **(c) Build the image in CI and have Coolify deploy a pre-built
  image** from a registry (Coolify's "Docker Image" build pack). CI
  already has both checkouts, so the context problem disappears
  entirely. This sidesteps rather than solves it, and you lose
  Coolify's git-push-to-deploy.
- **(d) Make the monorepo the deploy unit** — point Coolify at the
  parent repo (if `rust/` ever becomes one) with Base Directory
  `dev-pulse/`.

**Recommendation: (a) for the first deploy, (c) if builds turn out to
be slow or flaky on the Coolify host.** Note that Coolify builds on the
*target server*, not a remote builder farm — a 10–15 min Rust release
build will peg that box. That alone may push you to (c).

Decision needed from you before implementation starts.

### 2.2 `.dockerignore.fly` is written for the parent context

The whitelist (`!starter/**`, `!dev-pulse/**`) only makes sense when
the context is `rust/`. Whichever option above wins, this file gets a
sibling — `.dockerignore.coolify`, or just a corrected root
`.dockerignore` — that trims the same ~2.5 GB → ~22 MB. Coolify honours
a `.dockerignore` in the build context normally; there is no
`--ignorefile` equivalent to point somewhere else.

### 2.3 Bundled Postgres vs. Coolify's managed databases

Today Postgres 16 runs *inside* the app image, initdb'd onto the Fly
volume at `/data/pgdata`, started by `fly-entrypoint.sh`. The reason
was Fly-specific: Managed Postgres isn't offered in `cdg` and a
separate cluster doubled the bill.

**That reason does not apply on Coolify.** Coolify has first-class
Postgres as a resource type, on the same server, with S3 backups built
in — no extra cost beyond the RAM it uses. Keeping PG in the app image
on Coolify means you keep all the downsides (no redundancy, backup is
your problem, `pg_ctl` fights with the entrypoint on subcommands — see
FLY.md pitfall #9) and gain nothing.

So the scope includes: **split Postgres out into a Coolify Postgres
resource, and point `DATABASE_URL` at it.** The entrypoint's whole
`start_postgres` / `stop_postgres` machinery drops out of the Coolify
path. This is the change that makes the deploy genuinely simpler than
Fly's, and it's what makes the `*)` subcommand branch (`create-admin`,
`import-my-orgs`, `import-my-repos`) work without the two-Postgres
race.

The SQLite auth sidecar (`/data/auth.db`) stays, on a Coolify
persistent volume mounted at `/data`.

### 2.4 Everything is named `fly`

`Dockerfile.fly`, `config.fly.toml`, `fly-entrypoint.sh`,
`DP_*` defaults pointing at `dev-pulse.fly.dev`. None of it is *wrong*,
but a Coolify operator reading it will reasonably assume it's
Fly-only. Either generalise the names (`Dockerfile.deploy`,
`config.deploy.toml`, `entrypoint.sh`) and keep Fly working off the
same files, or fork a parallel set. Generalising is my recommendation —
the two runtimes want the same image; only the surrounding config
differs.

---

## 3. Proposed deliverables

| # | Deliverable | Notes |
|---|---|---|
| 1 | `Dockerfile.coolify` (or generalised `Dockerfile.deploy`) | Same multi-stage build, **minus** the Postgres 16 apt install. Keeps Caddy, tini, gettext-base, curl. Smaller and faster to build. |
| 2 | `scripts/entrypoint.sh` | Fork of `fly-entrypoint.sh` with `start_postgres`/`stop_postgres` removed. Still does envsubst → `/etc/dev-pulse/config.toml`, still runs `migrate` before serve, still `cd /app` for the authz policy path, still Caddy-background + backend-foreground under tini. |
| 3 | `crates/dev-pulse/config.coolify.toml` | Near-identical to `config.fly.toml`. Same `${VAR}` placeholders, same `secret://` handles. May be able to reuse `config.fly.toml` verbatim if renamed. |
| 4 | `.dockerignore` correction | Per §2.2, scoped to whatever context option §2.1 lands on. |
| 5 | `docker-compose.coolify.yml` *(optional)* | If we use Coolify's Compose build pack instead of the single-app Dockerfile pack, this declares app + postgres + volume in one file and Coolify reads it as the source of truth. See §4 for the trade-off. |
| 6 | `COOLIFY.md` | The runbook, mirroring FLY.md: bootstrap, env/secret table, admin seeding, org/repo import, scheduler enablement, verification, troubleshooting. |
| 7 | `Makefile` targets | `coolify-*` equivalents where they make sense (mostly local-image testing; Coolify deploys are git-push or API-triggered, not CLI). |

---

## 4. One design decision: Dockerfile pack vs. Compose pack

**Dockerfile build pack** — Coolify treats the app as one container.
Postgres is a *separate Coolify resource*, wired by `DATABASE_URL`.
Closest to the current Fly shape, and each piece gets Coolify's native
UI (backups on the DB, health checks on the app).

**Docker Compose build pack** — one `docker-compose.coolify.yml`
declares app + postgres together; Coolify parses it, surfaces
`${VARIABLE}` as editable fields, and can generate credentials via its
`SERVICE_PASSWORD_*` magic-variable syntax. More self-contained and
reviewable in git, but the Postgres instance is then a compose service
rather than a first-class Coolify database — **you don't get the
built-in S3 backup UI for it**, which is one of the main reasons to
move off the bundled-PG approach.

**Recommendation: Dockerfile pack + a managed Coolify Postgres
resource.** The backup story is the deciding factor.

---

## 5. Configuration mapping (what to set in the Coolify UI)

| Coolify setting | Value |
|---|---|
| Build Pack | Dockerfile |
| Base Directory | depends on §2.1 |
| Dockerfile Location | `Dockerfile.coolify` |
| Ports Exposes | `8080` |
| Health Check Path | `/health` |
| Domain | `https://<your-domain>` — Traefik terminates TLS |
| Persistent Storage | volume → `/data` (auth.db only, ~small) |

Environment variables — same names as Fly, three changes:

| Variable | Fly value | Coolify value |
|---|---|---|
| `DATABASE_URL` | auto-built, `127.0.0.1:5432` | **Coolify Postgres internal URL** |
| `DP_PUBLIC_BASE_URL` | `https://dev-pulse.fly.dev` | your Coolify domain |
| `POSTGRES_PASSWORD` | consumed by the entrypoint | **drops out** — managed by Coolify |

Unchanged: `DP_BIND_ADDR`, `DP_DEFAULT_RETURN`, `DP_AUTH_SQLITE_URL`,
`DP_SCHEDULER_ENABLE`, `DP_GITHUB_OAUTH_CLIENT_ID`, `RUST_LOG`, and the
secrets `GITHUB_PAT`, `GITHUB_WEBHOOK_SECRET`,
`OAUTH_GITHUB_CLIENT_SECRET`, `DP_GITHUB_ALLOW_ORGS`.

> The `DP_GITHUB_ALLOW_ORGS` JSON-array quoting trap (FLY.md pitfall
> #7) is a *shell* problem with `fly secrets set`. Pasting
> `["NubeIO","NubeDev","PJNube"]` into a Coolify UI field has no shell
> in the path, so it should just work — but it's still `envsubst`'d
> raw into TOML, so a malformed value still crash-loops the container.
> Worth a line in the runbook.

---

## 6. What carries over unchanged

Worth stating explicitly, because it's most of the risk surface and
none of it moves:

- **The Caddyfile.** Coolify's Traefik plays the exact role fly-proxy
  did — terminates TLS, forwards plain HTTP to `:8080`. `auto_https
  off` stays correct. The handle-block ordering fix (pitfall #4) stays
  correct.
- **Migrations in the entrypoint.** Coolify has no `release_command`
  either, so the existing "migrate before serve" placement is already
  the right shape.
- **The authz policy relative-path workaround** (pitfall #3) — image
  layout and `cd /app` are unchanged.
- **The pnpm react/@types/react overrides** (pitfall #6).
- **Rust MSRV pinning** (pitfall #2).
- **Post-deploy seeding** — `create-admin`, `import-my-orgs`,
  `import-my-repos` are the same commands; only the shell-in changes
  (`fly ssh console` → Coolify's browser terminal or `docker exec`).
  And per §2.3 they get *easier*, since there's no bundled PG to
  collide with.

---

## 7. Open questions for you

1. **§2.1 — how do we resolve the `starter` sibling dependency?**
   Submodule, registry publish, or CI-built image? This blocks
   everything else.
2. **Is the Coolify server sized for a Rust release build?** ~10–15 min
   and several GB of RAM, on the same box serving the app. If it's a
   small VPS, option (c) — build in CI, deploy the image — stops being
   optional.
3. **Do we keep Fly alive in parallel?** Determines whether we
   generalise the `*.fly.*` files or fork them.
4. **Custom domain and GitHub OAuth app** — new OAuth app for the
   Coolify origin, or re-point the existing one? Both can't share a
   callback URL, so running Fly and Coolify simultaneously means two
   OAuth apps.

---

## 8. Rough sizing

Assuming submodule (§2.1a) and Dockerfile pack + managed PG (§4):

| Task | Estimate |
|---|---|
| Vendor `starter` as submodule, prove the build in the new context | 0.5–1 day |
| `Dockerfile.coolify` + `entrypoint.sh` (de-Postgres'd) | 0.5 day |
| Coolify app + PG resource + env wiring, first green deploy | 0.5 day |
| Seeding, scheduler, custom domain, OAuth callback | 0.5 day |
| `COOLIFY.md` runbook | 0.5 day |
| **Total** | **~2.5–3 days**, plus whatever §2.1 turns into |

The estimate assumes the Coolify server already exists and is
provisioned. If option (b) — registry publishing — wins, add several
days and treat it as its own piece of work.
