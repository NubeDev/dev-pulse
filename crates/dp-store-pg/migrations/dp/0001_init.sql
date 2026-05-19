-- dev-pulse v1 schema.
--
-- Every table, index, and constraint required by the dp-domain Store
-- trait. Applied by `dp_store_pg::sources()` via the namespaced
-- runner in starter-store-postgres (progress lives in
-- `_sqlx_migrations_dp`, separate from any other migration source the
-- consumer registers — e.g. `starter_auth_users`).
--
-- Design notes worth knowing before editing:
--
--   * Events have NO `user_id` column (TODO §0.2). Attribution lives
--     in `event_actors` keyed by (event_id, user_id, role). This is
--     the only way co-authored commits and squash-merge author /
--     committer splits stay correct.
--   * `fetch_runs` is a run log only (TODO §0.3). Resume points live
--     in `fetch_cursors`, one per (org_id, repo_id, resource_kind).
--     `repo_id IS NULL` is meaningful for org-scoped resources
--     (members, teams) — we rely on PG15 `NULLS NOT DISTINCT` so the
--     unique constraint treats two NULL repo_ids as a conflict.
--   * Soft-delete + pseudonymisation rewrites `users.login/email/name`
--     in place and stamps `deleted_at`; the row id is never reused so
--     FK integrity on historical events holds (TODO §0.5).
--   * `activity_events.payload` and `webhook_inbox.payload` are JSONB
--     so the fetcher can store the trimmed projection (events) and
--     the raw delivery body (webhooks) without a schema change per
--     event kind.

-- ---------- users ----------------------------------------------------

CREATE TABLE dp_users (
    id          UUID         PRIMARY KEY,
    github_id   BIGINT       NOT NULL UNIQUE,
    login       TEXT         NOT NULL,
    email       TEXT         NULL,
    name        TEXT         NULL,
    deleted_at  TIMESTAMPTZ  NULL
);

-- Lookup-by-login is common (manual home-org mapping UI, webhook
-- attribution fallbacks). Index it; uniqueness is not enforced
-- because GitHub may rename a login and we want the rename to be
-- replayable.
CREATE INDEX dp_users_login_idx ON dp_users (login);

-- ---------- orgs / teams / repos ------------------------------------

CREATE TABLE dp_orgs (
    id         UUID    PRIMARY KEY,
    github_id  BIGINT  NOT NULL UNIQUE,
    login      TEXT    NOT NULL UNIQUE,
    name       TEXT    NULL
);

CREATE TABLE dp_teams (
    id         UUID    PRIMARY KEY,
    org_id     UUID    NOT NULL REFERENCES dp_orgs(id) ON DELETE CASCADE,
    github_id  BIGINT  NOT NULL,
    slug       TEXT    NOT NULL,
    name       TEXT    NOT NULL,
    UNIQUE (org_id, github_id),
    UNIQUE (org_id, slug)
);

CREATE TABLE dp_repos (
    id         UUID    PRIMARY KEY,
    org_id     UUID    NOT NULL REFERENCES dp_orgs(id) ON DELETE CASCADE,
    github_id  BIGINT  NOT NULL,
    name       TEXT    NOT NULL,
    UNIQUE (org_id, github_id),
    UNIQUE (org_id, name)
);

-- ---------- memberships ---------------------------------------------

CREATE TABLE dp_memberships (
    user_id    UUID         NOT NULL REFERENCES dp_users(id) ON DELETE CASCADE,
    org_id     UUID         NOT NULL REFERENCES dp_orgs(id)  ON DELETE CASCADE,
    role       TEXT         NOT NULL,
    home_org   UUID         NULL REFERENCES dp_orgs(id),
    joined_at  TIMESTAMPTZ  NOT NULL,
    PRIMARY KEY (user_id, org_id)
);

CREATE INDEX dp_memberships_org_idx       ON dp_memberships (org_id);
CREATE INDEX dp_memberships_home_org_idx  ON dp_memberships (home_org) WHERE home_org IS NOT NULL;

-- ---------- activity events + actors --------------------------------

CREATE TABLE dp_activity_events (
    id           UUID         PRIMARY KEY,
    org_id       UUID         NOT NULL REFERENCES dp_orgs(id)  ON DELETE CASCADE,
    repo_id      UUID         NOT NULL REFERENCES dp_repos(id) ON DELETE CASCADE,
    kind         TEXT         NOT NULL,
    ts           TIMESTAMPTZ  NOT NULL,
    external_id  TEXT         NOT NULL,
    payload      JSONB        NOT NULL DEFAULT '{}'::jsonb,
    UNIQUE (kind, external_id)
);

-- Report path: list_event_actor_rows_in_window joins event_actors to
-- events and filters by ts within a window, then optionally by org /
-- repo / user / role. These three indexes cover the common
-- access patterns; (org_id, ts DESC) is the dominant one. DESC
-- mirrors the TODO §Phase 1 spec — recent-events-first is the
-- universal report shape.
CREATE INDEX dp_activity_events_org_ts_idx  ON dp_activity_events (org_id,  ts DESC);
CREATE INDEX dp_activity_events_repo_ts_idx ON dp_activity_events (repo_id, ts DESC);
CREATE INDEX dp_activity_events_kind_ts_idx ON dp_activity_events (kind,    ts DESC);

CREATE TABLE dp_event_actors (
    event_id  UUID  NOT NULL REFERENCES dp_activity_events(id) ON DELETE CASCADE,
    user_id   UUID  NOT NULL REFERENCES dp_users(id)           ON DELETE CASCADE,
    role      TEXT  NOT NULL,
    PRIMARY KEY (event_id, user_id, role)
);

-- "Show me this user's activity" — reverse lookup from a user to
-- their event_actor rows. The join then fetches the event row via
-- its PK. The composite `(user_id, event_id)` matches the TODO
-- §Phase 1 mandatory-index list and lets index-only scans satisfy
-- the join without a heap fetch on event_actors.
CREATE INDEX dp_event_actors_user_event_idx ON dp_event_actors (user_id, event_id);
CREATE INDEX dp_event_actors_role_idx       ON dp_event_actors (role);

-- ---------- fetch runs + cursors ------------------------------------

CREATE TABLE dp_fetch_runs (
    id        UUID         PRIMARY KEY,
    kind      TEXT         NOT NULL,
    started   TIMESTAMPTZ  NOT NULL,
    finished  TIMESTAMPTZ  NULL,
    items     BIGINT       NOT NULL DEFAULT 0,
    errors    BIGINT       NOT NULL DEFAULT 0,
    partial   BOOLEAN      NOT NULL DEFAULT FALSE
);

CREATE INDEX dp_fetch_runs_started_idx ON dp_fetch_runs (started DESC);

CREATE TABLE dp_fetch_cursors (
    org_id         UUID         NOT NULL REFERENCES dp_orgs(id)  ON DELETE CASCADE,
    repo_id        UUID         NULL     REFERENCES dp_repos(id) ON DELETE CASCADE,
    resource_kind  TEXT         NOT NULL,
    since          TIMESTAMPTZ  NULL,
    etag           TEXT         NULL,
    last_event_id  TEXT         NULL,
    updated_at     TIMESTAMPTZ  NOT NULL,
    -- TODO §Phase 1 calls this a "composite PK (org_id, repo_id,
    -- resource_kind)". A literal PRIMARY KEY can't include
    -- `repo_id` because PG forbids NULL in PK columns and org-scoped
    -- resources (members, teams) intentionally carry NULL repo_id.
    -- We get the same semantics — exactly one row per
    -- (org, repo_or_org-scope, resource_kind) — via PG15+
    -- `NULLS NOT DISTINCT` on a unique constraint, which treats two
    -- NULL repo_ids as a conflict. Upserts in
    -- `dp_store_pg::PgStore::put_cursor` target this constraint.
    CONSTRAINT dp_fetch_cursors_pk
        UNIQUE NULLS NOT DISTINCT (org_id, repo_id, resource_kind)
);

-- ---------- webhook inbox -------------------------------------------

CREATE TABLE dp_webhook_inbox (
    id            UUID         PRIMARY KEY,
    delivery_id   TEXT         NOT NULL UNIQUE,
    event         TEXT         NOT NULL,
    payload       JSONB        NOT NULL,
    received_at   TIMESTAMPTZ  NOT NULL,
    processed_at  TIMESTAMPTZ  NULL,
    error         TEXT         NULL
);

-- Worker drain path: `SELECT ... WHERE processed_at IS NULL ORDER
-- BY received_at FOR UPDATE SKIP LOCKED`. The partial predicate
-- (`processed_at IS NULL`) keeps the index tiny once the inbox has
-- burned through most of its rows; ordering by `received_at` inside
-- that partial set gives FIFO drain. The TODO §Phase 1 mandatory
-- list spells this as `webhook_inbox(processed_at) WHERE
-- processed_at IS NULL` — same partial predicate, different leading
-- column. We pick `received_at` because the worker query orders by
-- it; indexing `processed_at` (which is NULL by definition inside
-- the partial set) would carry no information.
CREATE INDEX dp_webhook_inbox_pending_idx
    ON dp_webhook_inbox (received_at)
    WHERE processed_at IS NULL;

-- ---------- issues --------------------------------------------------

-- SCOPE §4.1: the local store models issues with **all** fields the
-- future CRUD-on-issues feature will need (title, body, labels,
-- assignees, state, milestone), not just the counters required for
-- reporting. Storing them now means no schema reshape later. Labels
-- and assignees are jsonb arrays so we can land the schema without
-- committing to a normalised label / assignee table — those can
-- arrive in a later migration if reports need them.
CREATE TABLE dp_issues (
    id           UUID         PRIMARY KEY,
    org_id       UUID         NOT NULL REFERENCES dp_orgs(id)  ON DELETE CASCADE,
    repo_id      UUID         NOT NULL REFERENCES dp_repos(id) ON DELETE CASCADE,
    github_id    BIGINT       NOT NULL,
    number       BIGINT       NOT NULL,
    title        TEXT         NOT NULL,
    body         TEXT         NULL,
    state        TEXT         NOT NULL,
    labels       JSONB        NOT NULL DEFAULT '[]'::jsonb,
    assignees    JSONB        NOT NULL DEFAULT '[]'::jsonb,
    milestone    TEXT         NULL,
    created_at   TIMESTAMPTZ  NOT NULL,
    updated_at   TIMESTAMPTZ  NOT NULL,
    closed_at    TIMESTAMPTZ  NULL,
    UNIQUE (repo_id, github_id),
    UNIQUE (repo_id, number)
);

-- Most issue queries are "list issues in this repo by recency" or
-- "list open issues in this org". Cover both.
CREATE INDEX dp_issues_repo_updated_idx ON dp_issues (repo_id, updated_at DESC);
CREATE INDEX dp_issues_org_state_idx    ON dp_issues (org_id,  state);

-- ---------- audit log ----------------------------------------------

-- SCOPE §9 transparency / §0.5 access-log requirement. Every
-- protected handler writes one row. `actor_user_id` stays a UUID
-- even after the user is pseudonymised (§0.5) — pseudonymisation
-- rewrites `dp_users.login/email/name` but keeps the `id`, which
-- preserves legal-defensibility of the log.
CREATE TABLE dp_audit_log (
    id             UUID         PRIMARY KEY,
    actor_user_id  UUID         NOT NULL REFERENCES dp_users(id),
    action         TEXT         NOT NULL,
    target         TEXT         NOT NULL,
    at             TIMESTAMPTZ  NOT NULL
);

CREATE INDEX dp_audit_log_actor_at_idx ON dp_audit_log (actor_user_id, at DESC);
CREATE INDEX dp_audit_log_at_idx       ON dp_audit_log (at DESC);
