## Done

- Implemented stage 3: auth shell + layout for the dev-pulse frontend.
- Added `<AuthProvider>` wiring (`sessionStrategy` from `@nube/starter-ui-core/auth`, sharing the `api.client` StarterClient from stage 1) in `frontend/src/app.tsx` and wrapped the tree in a `QueryClientProvider` in `frontend/src/main.tsx`.
- New files: `src/auth/strategy.ts` (re-exports `sessionStrategy` as `authStrategy`), `src/auth/login-page.tsx` (Card/Input/Label/Button form -> `auth.login({ kind: "credentials", … })`), `src/auth/protected-route.tsx` (loading shim + redirect to `#/login` on unauthenticated), `src/layout/app-shell.tsx` (top bar with email + role badge + Logout, left sidebar with Reports / Directory / Admin), `src/routes.ts` (tiny hash-based router via `useSyncExternalStore`).
- `src/app.tsx` orchestrates: AuthProvider → Router → either LoginPage or `<ProtectedRoute><AppShell><SectionPane/>`. Section panes are placeholder Cards; `ReportsHome` sketches the §8.1 three-lens Tabs so stage 4 can drop the real bodies in.
- `pnpm typecheck` clean, `pnpm build` produces dist/ (292KB JS / 11KB CSS).
- Committed as `stage 3: auth shell + layout — …`.

## Next

- Stage 4 should land the SCOPE §11.5 report pages (headline + table + trend, "Data as of <ts>", three-lens toggle wired to the api methods from stage 2). The `<Tabs>` skeleton + the QueryClientProvider are already in place; stage 4 should swap each `TabsContent` body for a real report view and add per-section routes (e.g. `#/reports/user/:id`) by extending `src/routes.ts`.

## What you need to know

- Router is hash-based (no react-router dep). Use `<a href="#/section/...">` for normal links; `navigate("/path")` is only used by the protected-gate redirect and the post-login push to `#/reports`.
- The `<AuthProvider>` reuses the `StarterClient` already created inside `api` (stage 1): `<AuthProvider client={api.client} strategy={authStrategy}>`. Don't construct a second StarterClient or the cookies vs typed-API calls will diverge.
- `sessionStrategy` already targets `POST /auth/login` + `GET /auth/me` + `POST /auth/logout` — the stage description mentioned `tokenStrategy` and `POST /auth/session`, but dp-server's Phase 4 surface is the `starter-auth-users` session router (`/auth/login`), and `sessionStrategy` is the cookie-login variant in `starter-ui-core`. I left a comment in `src/auth/strategy.ts` explaining the choice.
- `frontend/dist/` IS tracked in git (stage 0 committed it despite the gitignore comment claiming otherwise); the commit refreshes those artefacts. Consider adding `frontend/dist` to `.gitignore` in a later stage if that was unintended.

## Open questions

- Should GitHub OAuth get a "Continue with GitHub" button on the login page now, or wait until Phase 6 wires the SPA-facing OAuth round-trip? Stage 3 deliberately ships local email/password only.
- No test runner is configured in `frontend/` (no vitest / testing-library installed). The stage's "submitting credentials calls the session endpoint" smoke is currently verified only by typecheck + build + manual interaction; later stages may want to bring in vitest + msw.
