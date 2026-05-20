## Done

- Added `frontend/src/api/client.ts` — `DevPulseApi` wraps `StarterClient` and exposes typed methods for every dp-rest endpoint in the OpenAPI snapshot: `getReportFreshness`, `getReportUser/Team/Org`, `getReportHomeOrgSplit`, `listOrgs/Teams/Users`, `setHomeOrg`, `adminRefresh`, `listRuns`, `anonymiseUser`, `exportUser`.
- Zod schemas for every component (`Ack`, `CountRow`, `HomeOrgSplitRow`, `DataAsOfDto`, `ResolvedWindow`, `OrgDto`, `TeamDto`, `UserDto`, `MembershipDto`, `FetchRunDto`, `ExportEvent`, `UserExport`, `RefreshResponse` discriminated union, `SetHomeOrgRequest`) — types derived via `z.infer`.
- `ReportResponse<TRow>` generic; per-method `rows` narrowed to `CountRow[]` / `HomeOrgSplitRow[]` / `null`.
- `ReportParams` typed (`WindowLabel`, `WindowAnchor`, `ScopeMode`, `GroupBy` literal unions) with arrays joined to comma-separated query strings (matches `dp_rest::reports::ReportQuery`).
- Singleton `api` exported, baseUrl from `import.meta.env.VITE_API_BASE_URL` (empty default → Vite proxy / same-origin prod).
- Added `zod ^3.25.76` to `frontend/package.json`; `pnpm typecheck` passes.
- Committed as `07aa060` with the stage title prefix.

## Next

- Stage 3: wire `QueryClientProvider` + `AuthProvider` from `@nube/starter-ui-core` into `app.tsx`, using `api.client` for the login/me/logout calls.

## What you need to know

- Mutating methods (`setHomeOrg`, `adminRefresh`, `anonymiseUser`) read the `starter_csrf` cookie and echo it as `X-CSRF-Token` — same convention as `starter-client-ts`'s `auth.ts`.
- All requests use `credentials: "include"`; in dev the Vite proxy (`vite.config.ts`) forwards `/reports`, `/directory`, `/admin`, `/auth`, `/health`, `/openapi.json` to `http://localhost:3000`, so same-origin cookies work without CORS config.
- `exportUser` does a one-shot JSON parse — the server streams chunked, so for very large exports a future stage will want a streaming consumer.
- `ResolvedWindow` is a passthrough schema because dp-rest serialises `dp_domain::window::Window` verbatim and the OpenAPI snapshot leaves the shape open.
- `StarterClient` and `StarterError` are re-exported from `client.ts` so consumers have a single import path.

## Open questions

- (none)
