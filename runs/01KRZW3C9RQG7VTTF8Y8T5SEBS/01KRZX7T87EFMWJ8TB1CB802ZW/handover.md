## Done

- Added `dp-fetcher::worker` module with `Worker::{new, with_batch_size, with_idle_poll, drain_once, run}`, `DrainStats`, `WorkerError`.
- `apply_delivery` dispatches webhook deliveries to per-resource-kind handlers: pull_request (opened/closed/merged), pull_request_review (submitted), pull_request_review_comment (created), issues (opened/closed), issue_comment (created), push (with Co-authored-by trailer parsing), workflow_run (completed), deployment, release (published), member, membership, team.
- Multi-actor fan-out into `event_actors` with roles from the canonical union per §0.2. Squash-merge splits author/committer/merger; PR closer recorded from `sender`; issue closer recorded from `sender`; PR assignees + requested reviewers fan out.
- `worker::trailers` parses `Co-authored-by:` trailers case-insensitively; `noreply_login()` extracts GitHub `<id>+<login>@users.noreply.github.com` form.
- Bot accounts are recorded (login retains `[bot]` suffix); report-layer filtering is the right place for the SCOPE §6 "tracked separately" rule.
- Unattributed commits (missing `username`) record the `ActivityEvent` with no `Author` actor row.
- Each drain writes one `fetch_runs` row of kind `WebhookWorker` even when the batch is empty — used as "worker is alive" telemetry.
- Cooperative shutdown via `tokio::sync::watch::Receiver<bool>`; dropping the sender also exits the loop. Idle-poll select is the cancellation point.
- `worker::test_store::FakeStore` is a single in-memory Store fake reused by handler + drain tests. Unused Store methods panic so a future regression that touches them is loud.
- 28 new tests in `worker::*` pass; full crate test suite is 57 passing. Boundary check passes; workspace builds clean.

## Next

- Stage 6 per TODO §Phase 2: the 4h reconciler that compares local store vs GitHub via the octocrab client wrapper (conditional GETs from `fetch_cursors.etag` per §0.3) and fills gaps the webhook path missed; backfill is a later stage.

## What you need to know

- The worker uses `tokio::sync::watch::Receiver<bool>` for cancellation — `true` ⇒ shutdown; channel-closed ⇒ shutdown. Sender lives in `dp-server` (Stage 7/8 wiring).
- `HandlerError::Ignored` is the "well-formed but uninteresting" path (e.g. `pull_request.action = "labeled"`); the worker marks those processed so they leave the inbox.
- Push-commit authors carry `{name, email, username}` rather than a full user object. The worker uses `username` as login; missing username = unattributed (no Author actor). Co-authored-by trailers prefer the noreply login if present, else synthesise a row keyed off the email so re-runs collapse.
- Synthetic user `github_id` for login-only authors is the negated CRC32 of the login (negative keyspace; real GitHub ids are positive). The reconciler will overwrite via `upsert_user` once it has a real id.
- `record_event` is idempotent on `(kind, external_id)` — both in `dp-store-pg` and in `FakeStore`. `add_event_actors` dedups on the `(event_id, user_id, role)` composite PK.
- `cargo clippy -p dp-fetcher --all-targets -- -D warnings` fails on a pre-existing item in `client/mod.rs:12` from prior stages plus two new low-priority lints (`too_many_arguments` on `record`, `unnecessary_lazy_evaluations` on `or_else`). Not blocking; can be cleaned up in a follow-up.

## Open questions

- (none)
