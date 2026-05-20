# Docker

Full-stack dev-pulse in containers: Postgres + Rust backend + nginx-served SPA.

## Layout

| File | Purpose |
| --- | --- |
| [Dockerfile.backend](Dockerfile.backend) | Multi-stage Rust build → Debian slim runtime serving the API on `:8731`. |
| [Dockerfile.frontend](Dockerfile.frontend) | Multi-stage pnpm build → nginx serving the Vite SPA on `:80`. |
| [frontend/nginx.conf](frontend/nginx.conf) | nginx config: proxy backend routes, SPA fallback for everything else. |
| [docker-compose.yml](docker-compose.yml) | Wires postgres + backend + frontend together. |
| [crates/dp-store-pg/docker-compose.yml](crates/dp-store-pg/docker-compose.yml) | Postgres-only convenience for running `cargo test -p dp-store-pg`. |
| [.dockerignore](.dockerignore) | Keeps `target/`, `node_modules/`, `.env`, etc. out of build context. |

## Build context

The Cargo workspace and pnpm workspace both reference the sibling `../starter` checkout
(via `path = "../starter/..."` in [Cargo.toml](Cargo.toml) and `../starter/packages/*` in
[pnpm-workspace.yaml](pnpm-workspace.yaml)). Docker can only see files inside the build
context, so **the build context must be the parent directory** containing both repos:

```
/home/user/code/rust/
├── dev-pulse/   ← this repo
└── starter/     ← sibling checkout, required at build time
```

`docker-compose.yml` already sets `context: ..` so this is handled automatically.

## Quick start

```bash
# from the dev-pulse repo root
cp .env.example .env       # fill in GITHUB_PAT, GITHUB_WEBHOOK_SECRET, OAUTH_GITHUB_CLIENT_SECRET
docker compose up -d --build
```

| Service  | URL / Port                                       |
| -------- | ------------------------------------------------ |
| Frontend | http://localhost:8732                            |
| Backend  | http://localhost:8731                            |
| Postgres | `postgres://dev-pulse:devpass@localhost:5432/dev_pulse` |

Tail logs:

```bash
docker compose logs -f backend frontend
```

Stop everything:

```bash
docker compose down            # keeps the postgres volume
docker compose down -v         # ALSO drops the postgres data volume
```

## Just Postgres (for cargo tests)

```bash
cd crates/dp-store-pg
docker compose up -d
# then in another shell, from the repo root:
export DP_TEST_DATABASE_URL=postgres://dev-pulse:devpass@localhost:5432/dev_pulse
cargo test -p dp-store-pg -- --ignored
```

## Building images manually

If you'd rather not use compose, run from the **parent** directory:

```bash
cd /home/user/code/rust

docker build -f dev-pulse/Dockerfile.backend  -t dev-pulse-backend  .
docker build -f dev-pulse/Dockerfile.frontend -t dev-pulse-frontend .
```

## Secrets

The backend config uses `secret://NAME` handles that resolve to env vars (see
[crates/dev-pulse/src/main.rs](crates/dev-pulse/src/main.rs) — `resolve_secret`). Compose
loads `.env` from the repo root automatically and forwards the relevant keys into the
`backend` container:

- `GITHUB_PAT` — fetcher PAT (read-only ingest scopes)
- `GITHUB_WEBHOOK_SECRET` — HMAC for GitHub webhook verification
- `OAUTH_GITHUB_CLIENT_SECRET` — operator-login OAuth client secret

`.env` is gitignored — see [.env.example](.env.example) for the full list.

## Config

The backend image bakes in [crates/dev-pulse/config.example.toml](crates/dev-pulse/config.example.toml)
at `/etc/dev-pulse/config.toml` and runs `dev-pulse serve --config /etc/dev-pulse/config.toml`
by default. To use a different config, mount it over that path:

```yaml
services:
  backend:
    volumes:
      - ./config.local.toml:/etc/dev-pulse/config.toml:ro
```

## Notes & gotchas

- **First build is slow** (Rust release build of the whole workspace, ~minutes). Subsequent
  builds reuse Docker layer cache as long as `Cargo.lock` and source files don't change.
- The frontend image runs `pnpm install --frozen-lockfile=false` because the lockfile may
  drift across the dev-pulse + starter workspaces.
- nginx proxies match the dev-server proxy table in [frontend/vite.config.ts](frontend/vite.config.ts).
  If you add a new backend route prefix, update both.
- The backend container talks to postgres via the compose service name `postgres`, not
  `localhost`. The baked-in config's `url = "postgres://dev-pulse:devpass@localhost/dev_pulse"`
  will need either a mounted override or env-driven URL once that wiring lands.
