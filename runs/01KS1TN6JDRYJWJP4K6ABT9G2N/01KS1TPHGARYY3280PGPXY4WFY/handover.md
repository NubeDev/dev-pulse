## Done

- Scaffolded `frontend/` with Vite + React 18 + TypeScript (strict, `noUncheckedIndexedAccess`) + Tailwind v4.
- Root `package.json` + `pnpm-workspace.yaml` listing `frontend` and `../starter/packages/*` so `workspace:*` references to `@nube/starter-client-ts`, `@nube/starter-ui-core`, `@nube/starter-ui-kit` resolve.
- `frontend/vite.config.ts` proxies `/auth`, `/reports`, `/directory`, `/admin`, `/health`, `/openapi.json` to `localhost:3000` (dp-server); `@` alias points at the ui-kit src tree (mirrored in tsconfig paths).
- `frontend/src/globals.css` imports `@nube/starter-ui-kit/styles.css`; minimal `App` placeholder renders.
- Scripts: `dev`, `build` (tsc --noEmit + vite build), `preview`, `lint`, `typecheck`.
- `.gitignore` updated with `frontend/node_modules` and bare `node_modules`.
- Smoke verified: `pnpm install && (cd frontend && pnpm build)` produced `frontend/dist/` (index.html, ~143 kB JS, ~9 kB CSS).
- Committed as `f9c6acb` on `codeless/phase-7-frontend`.

## Next

- Stage 2: wire `AuthProvider` from `@nube/starter-ui-core/auth` + `QueryClientProvider`, add a simple email/password login screen against the `/auth/*` surface from starter-auth-users.

## What you need to know

- Repo layout assumes dev-pulse and starter are sibling checkouts (`../starter/packages/*`); same convention as `Cargo.toml`'s starter path deps.
- `@nube/starter-client-ts` codegens from `../../openapi.json` relative to its own package — that resolves to `starter`'s openapi snapshot, not dev-pulse's. Phase 4 needs to either copy dp-server's OpenAPI into starter's spot or extend the client; out of scope for this stage.
- pnpm warned about ignored build scripts (esbuild). Harmless for build; later stages may want `pnpm approve-builds`.
- Per task literal reading, only `node_modules` is git-ignored under `frontend/`. `frontend/dist/` is untracked but not in `.gitignore`; add it if/when desired.
- Dev proxy target is `localhost:3000` per task spec; confirm against actual dp-server listen port before Stage 2 if needed (current Rust crates didn't expose an obvious default in a quick grep).

## Open questions

- (none)
