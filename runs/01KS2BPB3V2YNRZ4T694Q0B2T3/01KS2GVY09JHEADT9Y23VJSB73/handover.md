## Done

- Extended `frontend/src/api/client.ts` with pins / tags / app-install-banner / issue write-path DTOs + methods, plus a `DpRestError` class that preserves `{status, code, body}` so callers can switch on the SCOPE-PROJECTS stable codes (`stale_local_version`, `writes_not_available_for_org`, `pin_cap_exceeded`, `batch_rejected`).
- New `frontend/src/workflow/` package: `pin-sidebar.tsx` (§6.1 render-cap "…and N more" overflow + tag expansion to viewer-filtered repos), `tags-page.tsx` (§7.4 default-scope logic + visible-link-count badges + archive-only retirement), `pins-page.tsx` (atomic §6.4 reorder + cap indicator), `issues-page.tsx` (CAS-on-version edit/comment/close/reopen with §8.3 reload UX), `writes-banner.tsx` (top-of-app §13.6 banner + per-form `WritesGate` for §8.4), `use-workflow-data.ts` (react-query hooks + `VITE_USE_MOCK_REPORTS=1` mocks), `mocks.ts` (mutable fixtures so the mock `updateIssue` reproduces the §8.3 stale-CAS race).
- Wired the new section into `routes.ts`, `app.tsx`, `layout/app-shell.tsx`. Added an `extraContent` slot on `AppSidebar` so the Pin widget mounts under the main nav without consumers needing to know about workflow.
- `pnpm typecheck` and `pnpm build` both clean.
- Committed as `stage 11: frontend wiring — …`.

## Next

- Stage 12 picks up. Likely: SCOPE-PROJECTS → SCOPE.md promotion (§13.1–§13.7 decisions + new sections in SCOPE.md), then the §15.6 envelope additive review.

## What you need to know

- `useEffect`-style stale-version handling lives entirely client-side via a `formKey` bump that drops controlled state and re-seeds from a refetched GET — exactly the §8.3 "reload and re-prompt" contract.
- Per-verb issue handlers (`POST /issues`, `PATCH /issues/{id}`, `POST /issues/{id}/comments`) are not yet wired server-side (the §8.2 primitives in `crates/dp-rest/src/issues.rs` are still standalone). The frontend calls the documented routes; under mocks the full UX is exercisable now, and the same wire shape will hit the routes once they land.
- The `WritesGate` and `WritesBanner` both read `useAppInstallBanner()` so the §8.4 affordance and the §13.6 one-shot prompt share one source of truth — no risk of one disagreeing with the other.
- Tag/repo names aren't joined-in yet — the sidebar + pins page show short ids (`Repo 00000000…`). A follow-up stage should hydrate target names via `/repos` and `/tags` listings.
- E2E smoke suite was not extended in this stage; existing tests should still pass (no surface they assert on was touched), but the new `data-testid` hooks (`pin-sidebar`, `pin-sidebar-overflow`, `writes-not-available-banner`, `stale-version-notice`, `stale-version-reload`, `create-tag-dialog`, `tag-link-count-*`, `issue-edit-card`, `writes-gate-*`) are ready for a follow-up Playwright test.

## Open questions

- Should the pin sidebar's `extraContent` mount be conditional on the user being authenticated past the protected-route gate, or is the `usePins` enabled-when-data-loads behaviour enough? Current code renders nothing during auth-loading.
