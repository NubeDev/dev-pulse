-- 0019_user_identities.sql  (users.md §4 Slice A, linear-projects-idea.md §3.0)
--
-- Multi-identity model: one dp-user can claim many GitHub identities.
-- Adds three tables — `dp_user_identities`, `dp_membership_identities`,
-- `dp_identity_link_pending` — and backfills the first from the
-- existing single-identity `dp_users.github_id` column.
--
-- The legacy `dp_users.github_id` column is *not* dropped here; it is
-- deprecated and read-sites migrate to `dp_user_identities WHERE
-- is_primary` in a follow-up. The drop ships in a separate migration
-- once no reader references the column.

-- ---------- dp_user_identities --------------------------------------

CREATE TABLE dp_user_identities (
    user_id        UUID        NOT NULL REFERENCES dp_users(id) ON DELETE CASCADE,
    github_user_id BIGINT      NOT NULL,
    github_login   TEXT        NOT NULL,
    is_primary     BOOLEAN     NOT NULL DEFAULT FALSE,
    linked_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    verified_via   TEXT        NOT NULL
                     CHECK (verified_via IN ('oauth', 'admin_link', 'rotation')),
    PRIMARY KEY (user_id, github_user_id),
    UNIQUE (github_user_id)
);

-- Login lookup (Directory search by `github_login`, webhook attribution).
-- Not UNIQUE because GitHub permits rename + reuse of a login over time.
CREATE INDEX dp_user_identities_login_idx
    ON dp_user_identities (github_login);

-- Exactly one primary identity per dp-user. Enforced at the index
-- level so concurrent writers cannot both flip is_primary = TRUE.
CREATE UNIQUE INDEX dp_user_identities_primary_idx
    ON dp_user_identities (user_id) WHERE is_primary;

-- ---------- dp_membership_identities --------------------------------
--
-- Per-identity provenance on memberships. Lets `unlink` subtract only
-- the orgs no remaining identity still covers. Cascades off the
-- identity row above via the composite FK so unlink "just works".

CREATE TABLE dp_membership_identities (
    user_id        UUID        NOT NULL,
    org_id         UUID        NOT NULL REFERENCES dp_orgs(id) ON DELETE CASCADE,
    github_user_id BIGINT      NOT NULL,
    observed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, org_id, github_user_id),
    FOREIGN KEY (user_id, github_user_id)
      REFERENCES dp_user_identities (user_id, github_user_id) ON DELETE CASCADE
);

CREATE INDEX dp_membership_identities_org_idx
    ON dp_membership_identities (org_id, user_id);

-- ---------- dp_identity_link_pending --------------------------------
--
-- Server-side OAuth `state` nonces for the link round-trip. The
-- session_id on the row binds the callback to the exact session that
-- started the link, preventing a cookie-rotation or actor-swap from
-- silently linking the wrong dp-user. See users.md §2.1.1.
--
-- TTL is enforced by the application (5 min). The expires_at index
-- supports a periodic GC sweep without a full scan.

CREATE TABLE dp_identity_link_pending (
    nonce       UUID        PRIMARY KEY,
    dp_user_id  UUID        NOT NULL REFERENCES dp_users(id) ON DELETE CASCADE,
    session_id  TEXT        NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL
);

CREATE INDEX dp_identity_link_pending_expires_idx
    ON dp_identity_link_pending (expires_at);

-- ---------- backfill from dp_users.github_id ------------------------
--
-- Every existing dp-user gets one primary identity row stamped from
-- their legacy `dp_users.github_id` / `dp_users.login`. `verified_via
-- = 'oauth'` since the legacy column was only ever populated by the
-- OAuth callback. Skip deleted users.

INSERT INTO dp_user_identities
    (user_id, github_user_id, github_login, is_primary, linked_at, verified_via)
SELECT
    u.id,
    u.github_id,
    u.login,
    TRUE,
    now(),
    'oauth'
FROM dp_users u
WHERE u.deleted_at IS NULL
ON CONFLICT (github_user_id) DO NOTHING;

-- Backfill provenance from the existing `dp_memberships` rows. We
-- attribute every membership to the user's (now-primary) identity so
-- the invariant "memberships exist iff at least one provenance row
-- exists" holds at the moment 0019 finishes. Follow-on stamper ticks
-- refine this when a user grows secondary identities.

INSERT INTO dp_membership_identities
    (user_id, org_id, github_user_id, observed_at)
SELECT
    m.user_id,
    m.org_id,
    u.github_id,
    m.joined_at
FROM dp_memberships m
JOIN dp_users u ON u.id = m.user_id
WHERE u.deleted_at IS NULL
ON CONFLICT (user_id, org_id, github_user_id) DO NOTHING;
