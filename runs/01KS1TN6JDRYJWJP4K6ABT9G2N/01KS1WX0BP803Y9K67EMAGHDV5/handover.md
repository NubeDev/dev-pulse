## Done

- Added directory section with four pages mounted under `#/directory/{users,orgs,teams,home-org}`: Users list (search + org filter + memberships + home_org badge), Orgs list (member count), Teams list (filtered by org), and Home-org assignment UI (POST /home-org with AlertDialog confirmation + optimistic update + rollback on failure).
- New `frontend/src/directory/` module: `use-directory.ts` (shared react-query data hook + optimistic home-org store), `mocks.ts`, `users-page.tsx`, `orgs-page.tsx`, `teams-page.tsx`, `home-org-page.tsx`.
- `routes.ts` gained `DirectoryTab` + `directoryTabOf()` mirroring `reportTabOf()`.
- `app.tsx` replaced the `DirectoryHome` placeholder with `DirectorySection` + plain-anchor sub-nav.
- `pnpm typecheck` and `pnpm build` are green; commit landed with the stage-7 title prefix.

## Next

- (none) — stage 8 picks up in a fresh session.

## What you need to know

- Memberships are derived client-side by fanning `GET /users?org_id=…` over every org from `GET /orgs` (no read-only memberships endpoint exists).
- `home_org` is also not exposed read-only (only via the audited `exportUser` admin endpoint), so the badge column reads from an in-process optimistic map seeded empty (or by mock fixture). The setHomeOrg mutation updates it via `onMutate` and rolls back via `onError`. When/if a `GET /memberships` (or `home_org` on `UserDto`) lands, swap the map for that source.
- Smoke mode (`VITE_USE_MOCK_REPORTS=1`) short-circuits every directory query to deterministic fixtures (3 orgs, 6 users, 2 pre-seeded home_orgs) — same flag the report pages use.
- The home-org dialog gates the org dropdown to the selected user's actual memberships; the server's 404 on non-existent membership is still the source of truth.

## Open questions

- Should a read-only memberships/home_org endpoint be added in a later phase so the badge column reflects server state across sessions? (Currently flagged in the use-directory.ts header.)
