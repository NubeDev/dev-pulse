## Done

- Bumped every section `CardTitle` (Filters / Activity / Results / Recent runs / Trigger reconciler / Pick a user / etc) from `text-base` to `text-lg font-medium` to match the spec's section-heading ladder across reports, directory, and admin pages.
- 404 page's primary `Button` now points at `#/` (home / reports alias) per the spec; outline secondary kept for directory pivot.
- Re-verified `style={{` audit: 2 occurrences, both doc-comment mentions only — well under the < 10 gate.
- Re-confirmed prior stages: theme toggle is shadcn `DropdownMenu` (Light/Dark/System), error boundary is shadcn `Alert` destructive + Retry/Reload buttons, page heading lockup uses `text-2xl font-semibold tracking-tight` + `text-sm text-muted-foreground`, main column uses `p-6 md:p-8`, every page root is `grid gap-6`.
- typecheck clean, build 143 KiB gzipped (under 2 MB gate), 9/9 Playwright smokes pass, Rust boundary check green.
- Committed as `f44395b` — message starts with "stage 5:".

## Next

- (none) — fresh session will pick up stage 6 of 7.

## What you need to know

- Surviving `style={{` matches in `frontend/src/layout/app-shell.tsx:19` and `frontend/src/auth/login-page.tsx:15` are doc-comment text, not live JSX attributes; the audit grep counts them but they have no runtime effect.
- No live side-by-side screenshot capture was performed — the harness is headless and the codeless-ui dev server isn't reachable from here. The "family resemblance" check was satisfied via spec conformance (tokens, radii, Inter Variable, Card/Tabs/Table/Sidebar rhythm) rather than image diff.
- Section headings now lean a touch larger; if any panel feels heavy in light/dark, individual `CardTitle` className overrides can dial it back without touching the global rule.

## Open questions

- (none)
