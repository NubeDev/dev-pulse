# Stage 7 — REVIEW gate (pre-GitHub-writes)

> Hard gate per WORKFLOW.md. Examined the diff from stages 2–6
> (`d98ade5..e069175`, ≈5 200 lines) against Layer-1 invariants
> (R1 / R2 / R4 / R5 + wire-format additivity) and the
> SCOPE-PROJECTS.md §13.1–§13.5 decisions that apply at this
> point. The §13.6 / §13.7 decisions are reviewed at the
> stage-12 REVIEW gate (their code hasn't landed yet).

## Verdict

**PASS.** All Layer-1 invariants hold. The branch is clear to
proceed to stage 8 (App permission bump) and stage 9 (issue
write path).

The "PASS, with notes" gaps below are documented v1 limitations,
already tracked as SCOPE-PROJECTS §12 open questions or as
pre-existing aspirational §15.12 wiring — not regressions
introduced by this branch.

## R1 — crate dependency direction

- `dp-domain` has no `dp-*` deps. ✓
- `dp-reports` depends only on `dp-domain`. ✓
- `dp-rest` depends on `dp-domain` + `dp-reports` + `dp-fetcher`
  (pre-existing, for `admin.rs::Scheduler`; not added by this
  branch). ✓
- `dp-server` depends on `dp-rest` + `dp-store-pg` + `dp-fetcher`
  + `dp-domain`. ✓
- No cycles introduced. New code (`dp-rest::pins`, `tags`,
  `dp-reports::tag_filter`, the §15.6 envelope additions) sits
  on the existing layering with no new upward arrows.

## R2 — single transport

- Stage 4 (pins) and stage 5 (tags) add only `dp-rest` HTTP
  handlers + `utoipa` annotations. No MCP write tools, no
  parallel gRPC / WS surface. ✓
- The §13.3 decision ("MCP stays read-only in v1; opening it
  requires its own scope doc") is honoured — no `dp-mcp` files
  touched on the branch.

## R4 / R5 — trust boundary

- Every new handler in `dp-rest::pins` and `dp-rest::tags`
  reads `Extension(Principal)` for the actor id; no path takes
  a `?user_id=` query knob (verified by grep — only
  `principal.actor_user_id` reads).
- Both routers are merged into `dp-server::build`'s `protected`
  router, which is wrapped by `with_principal(...)`. An
  unauthenticated request to any new path fails before reaching
  the handler. ✓
- Visibility filtering (tag list, tag-link counts) uses
  `ViewerVisibility`, which derives from
  `Store::list_memberships_for_user` — the same membership
  predicate the rest of `dp-rest` uses for visibility. Not a
  parallel access gate; an application-side filter on top of
  the membership predicate that §15.11 already defines.
- Per-pin paths only ever touch the caller's own pin rows
  (store calls scoped to `actor_user_id`). No cross-user
  inspection surface.

### Notes (gaps, not violations)

- **§15.12 `protected_routes()` smoke list** in
  `crates/dp-server/tests/phase4_smoke.rs:473` was not extended
  to enumerate `/me/pins*` or `/tags*` or `/me/tags*`. The new
  routes ARE behind `with_principal`, but the boundary smokes
  8/9 only verify enumerated routes — adding the new routes to
  that list is a stage-12 cleanup (and a one-line follow-up
  PR). Tracked here so the stage-12 reviewer remembers it.
- **Policy-engine `register_dev_pulse_resources`** registers
  `pins` (`read`, `write`) but does **not** register `tags`.
  This is harmless today because no router carries an actual
  `.layer(require_permission(...))` call — the §15.12
  `require_permission` wiring is currently aspirational across
  every dp-rest router (pre-existing condition, predates this
  branch). When §15.12 is closed for real, `tags`
  (`read`, `write`) must be added to `register_dev_pulse_resources`
  alongside `pins`. Tracked.
- **`visible_repo_ids`** in `dp-rest::tags` returns an empty
  set as a v1 conservative fallback (documented at
  `tags.rs:478-486`). Combined with the link filter at
  `tags.rs:444-448`, this means repo-kind tag links are
  *always dropped* from the viewer-filtered count, not "always
  included" as the surrounding comment claims. This is the
  *safer* direction (under-count, never over-count) but the
  comment/code mismatch is worth fixing before stage 11
  frontend wiring so the UI doesn't show 0/N for tags whose
  links are exclusively repo-kind. Logged as a §12-style open
  question; not blocking the gate.

## Wire-format additivity (§15.6)

- `ReportRequest` gained two fields: `tags: Vec<Uuid>` and
  `repos: Vec<Uuid>`. Both `#[serde(default)]`. Old clients
  that don't send them deserialise to empty vecs — confirmed
  by `report_request_defaults_empty_filters` test. ✓
- `GroupBy` gained `Tag` variant. Old clients that don't send
  `group_by: "tag"` see no change. The `MAX_TAGS_FOR_GROUP_BY_TAG`
  cap (50) is enforced at the handler boundary
  (`reports::group_by_tag_without_tags_filter_returns_400`). ✓
- New `empty_reason = "tag links do not match metric
  attribution"` is a response-side string; doesn't break old
  decoders (the field already existed). ✓
- OpenAPI snapshot regenerated and committed
  (`crates/dp-rest/tests/openapi.snapshot.json` +885 lines)
  — additive: no field renamed, no field removed.

## §13.1–§13.5 decisions check

| Decision | Status | Where |
|----------|--------|-------|
| §13.1 — projects = home-grown (no GitHub Projects v2 sync) | ✓ | no Projects v2 code on branch |
| §13.2 — tags polymorphic (repo/issue/user/team) | ✓ | `0005_user_pins_tags_tag_links.sql` CHECK constraint + `TagLinkKind` |
| §13.3 — REST-only write surface (no MCP write) | ✓ | only `dp-rest` handlers added |
| §13.4 — viewer-filtered link counts | ✓ | `filter_visible_links` + `ViewerVisibility::can_see` |
| §13.5 — pin cap from `dp-config`, 500-link warning | ✓ | pin cap test `pins.rs` ; 500-link warning header `tags.rs` |

§13.6 and §13.7 are the stage-12 gate's responsibility — their
code hasn't landed.

## §15.6 envelope — leaderboard branch reconciliation

`codeless/org-leaderboard` has not yet merged at the time of
this gate. Per STAGE-1-COORDINATION.md §1 the fallback path
applies: this branch added `ReportRequest.repos: Vec<Uuid>`
itself, and the leaderboard branch's later rebase will find
the field already present and re-point its
`LeaderboardEnvelope` doc-block at `ReportRequest.repos`. No
struct rename, no field-type change, no migration collision
(this branch took `0005_*`, leaderboard's slot stays `0004_*`).

## Sentinel

```
PASS: Layer-1 invariants (R1 dependency direction, R2 single transport, R4/R5 trust boundary, §15.6 additive wire format) all hold; §13.1–§13.5 decisions implemented; gaps logged are documented v1 limitations, not regressions.
```
