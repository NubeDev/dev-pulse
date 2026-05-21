# Local HTTPS Testing

## Problem

The Fly deployment runs the dev-pulse stack (Caddy + Rust backend +
SPA) behind HTTPS. Several flows only work correctly under HTTPS with
a matching origin:

- **GitHub OAuth callback** — GitHub redirects to whatever
  `redirect_uri` the start endpoint sent. If `DP_PUBLIC_BASE_URL` is
  `http://localhost:8732` (the [DOCKER.md](DOCKER.md) stack) but the
  OAuth App is registered with `https://...`, the callback 401s.
- **Secure session cookies** — `starter-auth-users` sets cookies with
  `Secure; HttpOnly; SameSite=Lax` once the origin is `https://`.
  Plain HTTP drops them silently and the user appears signed-out on
  the very next request.
- **CORS / `Origin` headers** — the browser sends `Origin: https://...`
  on form posts. Anything that compares origins (CSRF tokens,
  `SameSite` enforcement, OAuth state) only behaves right under TLS.
- **Mixed content** — the SPA loaded over HTTPS refuses to `fetch()`
  against `http://` API URLs. Hard-coded `http://` in any asset = a
  silent failure that local plain-HTTP never reveals.

Today the [DOCKER.md](DOCKER.md) compose stack runs plain HTTP on
`:8732` with no TLS terminator. OAuth-callback / cookie issues only
surface after a ~10–20 min `fly deploy`.

## Goal

Run the **exact same Docker image we deploy to Fly** locally, behind
real TLS on `https://localhost`. Catch HTTPS-only regressions in
~30 s, not after a deploy cycle.

## Approach

```
┌──────────────────────────────────────────────────┐
│  caddy-tls (mkcert-issued localhost cert)        │
│  https://localhost:443  →  app:8080              │
└──────────────────────────────────────────────────┘
         │
         ▼
┌──────────────────────────────────────────────────┐
│  app container (dev-pulse-fly:local)             │
│  fly-entrypoint → Caddy(:8080) → backend(:8731)  │
│                              ↘  /usr/share/dev-  │
│                                 pulse/web (SPA)  │
└────────────────────┬─────────────────────────────┘
                     │
                     ▼
              ┌─────────────────┐
              │ postgres:16     │
              │ (compose-only)  │
              └─────────────────┘
```

- **Outer Caddy** terminates TLS using a `mkcert`-issued cert
  (`certs/localhost.pem` + `certs/localhost-key.pem`). After
  `mkcert -install` the browser trusts it natively — no warnings,
  no `--insecure`.
- **The `app` container runs the canonical Fly image** —
  `dev-pulse-fly:local`, built from [Dockerfile.fly](Dockerfile.fly).
  Same binary, same entrypoint, same internal Caddy as production.
- **No bind-mounts** of source code into `app`. Every change to
  `Dockerfile.fly`, `Caddyfile`, `scripts/fly-entrypoint.sh`, or
  `crates/dev-pulse/config.fly.toml` lands in the image via the next
  `make fly-local` (Docker layer cache keeps incremental rebuilds
  short). This kills the "config ahead of image" drift class of bugs.
- Runtime origin is fully driven by `DP_PUBLIC_BASE_URL`. The
  entrypoint substitutes it into the runtime config; nothing in the
  image is hard-coded to `dev-pulse.fly.dev`.
- A named Docker volume (`dev-pulse-fly-local-data`) persists `/data`
  (the sqlite auth sidecar). Wipe with `make fly-local-reset` to
  reseed from scratch.
- A second named volume (`dev-pulse-fly-local-pgdata`) persists the
  local Postgres. Same wipe target clears both.

### Caveats not eliminated

- **Cert trust in-container.** mkcert's root CA is trusted by the host
  browser only. The `app` container's loopback traffic between Caddy
  and the backend is plain HTTP regardless — this is fine for OAuth /
  cookie testing (the public surface is what TLS protects), and
  matches what Fly's edge does in production.
- **Privileged port 443.** Caddy runs in a container, so this is
  handled by the compose port mapping. If `:443` is already taken on
  your host, fall back: edit the compose file's `caddy-tls` port to
  `8443:443` and run with `DP_PUBLIC_BASE_URL=https://localhost:8443`.
- **Hostname is `localhost`.** Simpler — no `/etc/hosts` edit. If you
  need a different hostname (e.g. to match a real subdomain), add
  `127.0.0.1 dev-pulse.local` to `/etc/hosts`, regenerate the cert
  with `mkcert dev-pulse.local`, edit `Caddyfile.local`, and set
  `DP_PUBLIC_BASE_URL=https://dev-pulse.local`.
- **Not a Fly-infra test.** Does not exercise fly-proxy header
  injection, regions, MPG, or volume snapshotting. Deploy to a
  staging Fly app for those.

## Files

| File | Purpose |
|---|---|
| [docker-compose.fly-local.yml](docker-compose.fly-local.yml) | `caddy-tls` + `app` + `postgres` services + named volumes. Zero bind-mounts of source. |
| [Caddyfile.local](Caddyfile.local) | `localhost { tls /certs/localhost.pem … reverse_proxy app:8080 }`. |
| [certs/.gitignore](certs/.gitignore) | Ignores `*.pem` so the mkcert cert never lands in git. |
| [Dockerfile.fly](Dockerfile.fly) | Canonical image — same one `fly deploy` ships. |
| [Makefile](Makefile) | `fly-local`, `fly-local-down`, `fly-local-reset`, `fly-local-logs`, `fly-local-build`. |

## Usage

### One-time setup

```bash
# 1. Install mkcert + trust its root in your browser store.
sudo apt install -y mkcert libnss3-tools
mkcert -install

# 2. Generate the localhost cert.
cd certs
mkcert localhost 127.0.0.1 ::1
mv localhost+2.pem     localhost.pem
mv localhost+2-key.pem localhost-key.pem
cd ..

# 3. Register a SEPARATE GitHub OAuth App for local testing.
#    GitHub → Settings → Developer settings → OAuth Apps → New
#    - Homepage URL          https://localhost
#    - Authorization callback URL  https://localhost/auth/oauth/github/callback
#    Copy the client ID + secret.

# 4. Write the local env file (gitignored).
cat > .env.fly-local <<'EOF'
DP_GITHUB_OAUTH_CLIENT_ID=Iv1.local...
OAUTH_GITHUB_CLIENT_SECRET=...
GITHUB_WEBHOOK_SECRET=local-dev-webhook-secret-not-secret
GITHUB_PAT=ghp_your_pat
DP_GITHUB_ALLOW_ORGS=["NubeIO"]
EOF
```

### Bring up the stack

```bash
make fly-local            # always rebuilds the image (Docker cache makes it ~30s)
```

Then open **<https://localhost/>** in Chrome / Firefox — no cert
warning if `mkcert -install` worked.

### Tear down

```bash
make fly-local-down       # stop, keep the volumes for fast restart
make fly-local-reset      # stop + wipe both volumes (Postgres + sqlite auth)
```

### Logs

```bash
make fly-local-logs                                        # both services
docker compose -f docker-compose.fly-local.yml logs -f app # just the backend image
```

## What this tests that the plain compose stack cannot

| Scenario | [docker-compose.yml](docker-compose.yml) (HTTP) | fly-local (HTTPS) |
|---|---|---|
| GitHub OAuth callback round-trip | ✗ OAuth App URL mismatch | ✓ |
| `Secure` session cookie set + replayed | ✗ browser drops `Secure` cookies over HTTP | ✓ |
| `SameSite=Lax` on cross-site form posts | ✗ HTTP cookies are SameSite=Lax-of-no-effect | ✓ |
| Mixed-content fetch from SPA | ✗ no HTTPS in front | ✓ |
| Full Fly-image regression (missing file, bad path in Dockerfile) | ✗ uses [Dockerfile.backend](Dockerfile.backend) / [Dockerfile.frontend](Dockerfile.frontend) | ✓ uses [Dockerfile.fly](Dockerfile.fly) |
| Webhook HMAC verification (with TLS-only X-Forwarded-Proto) | partial | ✓ |
| fly-proxy header injection / regions / MPG | ✗ | ✗ (deploy to Fly) |

## Iterating on a running stack

The named volumes are preserved by `make fly-local` (only
`make fly-local-reset` wipes them):

```bash
# Edit Dockerfile.fly / Caddyfile / fly-entrypoint.sh / SPA / Rust …
make fly-local            # rebuilds image, recreates app container, keeps volumes
```

Every run is a fresh container against the same Postgres + sqlite
state. Sessions and users survive across rebuilds.

## Verifying it's working

```bash
# Cert is trusted (no warning)
curl -sI https://localhost/health | head -1
# → HTTP/2 200

# OpenAPI doc via Caddy(TLS) → app:8080 → backend:8731
curl -s https://localhost/openapi.json | python3 -m json.tool | head -5

# SPA index
curl -s https://localhost/ | grep -oE '<title>[^<]+</title>'

# Session cookie carries Secure flag
curl -sI -c /tmp/c.txt https://localhost/auth/whoami | grep -i set-cookie
# → set-cookie: …; HttpOnly; Secure; SameSite=Lax
```

If `curl` complains about the cert, mkcert's root isn't trusted in the
**system** store — try `mkcert -install` again, or pass `--cacert
"$(mkcert -CAROOT)/rootCA.pem"`.

## Non-goals

- Not a replacement for `make start` (the day-to-day dev loop with
  hot-reloaded Vite + cargo-watch). That's faster for code iteration.
- Not a CI gate — this is a developer-local pre-flight before
  `make fly-deploy`. (A future CI job could run this same compose
  file headlessly + a Playwright smoke suite if it earns its keep.)
- Not testing Fly-specific infra. For that, deploy to a staging Fly
  app.
