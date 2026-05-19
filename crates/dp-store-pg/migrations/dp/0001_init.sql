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
-- access patterns; (org_id, ts) is the dominant one.
CREATE INDEX dp_activity_events_org_ts_idx  ON dp_activity_events (org_id, ts);
CREATE INDEX dp_activity_events_repo_ts_idx ON dp_activity_events (repo_id, ts);
CREATE INDEX dp_activity_events_kind_ts_idx ON dp_activity_events (kind, ts);

CREATE TABLE dp_event_actors (
    event_id  UUID  NOT NULL REFERENCES dp_activity_events(id) ON DELETE CASCADE,
    user_id   UUID  NOT NULL REFERENCES dp_users(id)           ON DELETE CASCADE,
    role      TEXT  NOT NULL,
    PRIMARY KEY (event_id, user_id, role)
);

-- "Show me this user's activity" — reverse lookup from a user to
-- their event_actor rows. The join then fetches the event row via
-- its PK.
CREATE INDEX dp_event_actors_user_idx ON dp_event_actors (user_id);
CREATE INDEX dp_event_actors_role_idx ON dp_event_actors (role);

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
    -- PG15+: treat NULL repo_id as equal for uniqueness so the
    -- org-scoped resources (members, teams) get at most one cursor
    -- per (org, resource_kind).
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

-- Worker drain path: `SELECT ... WHERE processed_at IS NULL FOR
-- UPDATE SKIP LOCKED`. Partial index keeps it tiny once the inbox
-- has burned through most of its rows.
CREATE INDEX dp_webhook_inbox_pending_idx
    ON dp_webhook_inbox (received_at)
    WHERE processed_at IS NULL;
