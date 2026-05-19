## Done

- crates/dp-store-pg/migrations/dp/0001_init.sql: added dp_issues (id, org_id, repo_id, github_id, number, title, body, state, labels jsonb, assignees jsonb, milestone, created_at, updated_at, closed_at + UNIQUE(repo_id,github_id), UNIQUE(repo_id,number) + dp_issues_repo_updated_idx, dp_issues_org_state_idx) per SCOPE §4.1 CRUD-fields requirement.
- Added dp_audit_log (id, actor_user_id, action, target, at) + dp_audit_log_actor_at_idx, dp_audit_log_at_idx per SCOPE §9 transparency/§0.5.
- Replaced dp_event_actors_user_idx (single-col) with dp_event_actors_user_event_idx (user_id, event_id) — matches TODO §Phase 1 mandatory-index list, enables index-only join path.
- Switched dp_activity_events_{org,repo,kind}_ts_idx to DESC ordering on ts (literal §Phase 1 spec match).
- Renamed the fetch_cursors UNIQUE constraint to dp_fetch_cursors_pk and expanded the comment explaining why "composite PK" is implemented as UNIQUE NULLS NOT DISTINCT (PG forbids NULL in PK columns; org-scoped resources need NULL repo_id).
- dp_webhook_inbox_pending_idx kept (partial WHERE processed_at IS NULL); comment now notes why we chose `received_at` over `processed_at` as the leading column (worker query orders by received_at FIFO; processed_at is uniformly NULL inside the partial set).
- Verified by applying the migration to a fresh postgres:16-alpine container: all 12 dp_* tables created without error (dp_users, dp_orgs, dp_teams, dp_repos, dp_memberships, dp_activity_events, dp_event_actors, dp_fetch_runs, dp_fetch_cursors, dp_webhook_inbox, dp_issues, dp_audit_log).
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `./scripts/check-boundaries.sh` all green.
- Committed as `stage 6: schema migrations — …` (8d1b2ef) on codeless/bootstrap-domain-store. Not pushed (no remote auth in this session, consistent with prior stages).

## Next

- Stage 7 is the REVIEW gate. Write the handover with the schema diagram (text fine) and confirm answers to the open questions in SCOPE.md. Do not auto-advance.
- After approval, stage 8 wires dp_store_pg::sources() into the dp-server starter migration runner and (likely) adds an integration test against a live PG.

## What you need to know

- `dp_issues` is a Phase 1 schema-only landing — there is no Issue entity in dp-domain and no Store method that touches it yet. That's deliberate per SCOPE §4.1 ("we don't have to use them, but we shouldn't have to re-shape the schema later"). When the issue CRUD work lands, the schema is ready.
- `dp_audit_log` likewise has no Store method yet. Phase 2/3 will add audit_log writes from protected handlers.
- The fetch_cursors deviation from a literal PRIMARY KEY is documented inline in the migration. If the reviewer prefers a true composite PK, the path is: change repo_id to NOT NULL with a sentinel UUID (e.g. all-zeroes for "org-scope") and update PgStore::{get,put}_cursor to coalesce None → sentinel. That's a stage-5 code change, not stage 6.
- Activity-events indexes are now DESC. PG btrees scan either direction, so ASC indexes would have worked too, but the literal §Phase 1 spec says DESC.
- The webhook_inbox partial index column choice (received_at vs the literal-spec processed_at) is justified in the SQL comment. If reviewer insists on processed_at literally, swap the leading column — it'd be useless (all NULL inside the partial set) but no test breaks.

## Open questions

- (carried, stage 7 review item) fetch_cursors "composite PK" — accept UNIQUE NULLS NOT DISTINCT as equivalent, or refactor to true PK with sentinel UUID for org-scoped repo_id?
- (carried) ActivityEvent.payload — trimmed projection vs raw GitHub body. Schema stores JSONB either way.
- (carried) Integration test strategy for stage 8: testcontainers vs DEV_PULSE_TEST_DATABASE_URL.
- (new) `dp_issues.labels` / `assignees` as JSONB arrays vs normalised label/assignee tables — fine for v1 reporting, may need normalisation when issue CRUD UI lands and we want to filter "show issues assigned to user X".
