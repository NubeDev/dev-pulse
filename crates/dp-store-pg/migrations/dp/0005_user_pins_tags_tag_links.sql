-- Workflow surface — per-user pins, cross-org home-grown tags, and the
-- polymorphic tag→target edges (SCOPE-PROJECTS.md §6.3 + §7.2).
--
-- Migration-numbering convention is locked in
-- `.codeless/jobs/projects-issues/STAGE-1-COORDINATION.md`:
--
--   * org-leaderboard owns evens   (0004, 0006)
--   * projects-issues  owns odds   (0005, 0007)
--
-- Hence `0005_user_pins_tags_tag_links.sql` here. The CAS / version /
-- pending_remote columns on `dp_issues` and the `dp_issue_mutations`
-- audit table are reserved for `0007_issues_optimistic_cas.sql` in a
-- later stage of this same job.
--
-- Three tables land here:
--
--   * `dp_user_pins`  — per-user ordered favourites over (repo|tag).
--   * `dp_tags`       — home-grown cross-org grouping primitive,
--                       scope ∈ {user, team, org}.
--   * `dp_tag_links`  — polymorphic edge tag → (repo|issue|user|team).
--
-- Decisions worth knowing before editing:
--
--   * `position` on `dp_user_pins` is **not** uniqued at the DB
--     level. §6.3 spells this out: "unique (user_id, position)
--     enforced at write time, not as a DB constraint, to allow
--     atomic reorder." Reorder rewrites every row in one tx and
--     would deadlock on a unique constraint.
--   * `dp_tags` uses **three nullable scope_*_id columns plus a
--     CHECK** rather than one `scope_id UUID` — that buys us real
--     `REFERENCES … ON DELETE CASCADE` per kind without modelling
--     a polymorphic FK in application code. Same trick for
--     `dp_tag_links` (four nullable `target_*_id` columns + CHECK).
--   * Case-insensitive per-scope name uniqueness is an **expression
--     index on `lower(name)`**, not a `UNIQUE` column-list — column-
--     list uniqueness would be case-sensitive ("Phoenix" vs
--     "phoenix" would both be allowed).
--   * The expression index COALESCEs the three scope_*_id columns to
--     a single non-NULL UUID per row. The CHECK guarantees exactly
--     one is non-NULL, so the COALESCE is total.
--   * `archived_at` is soft-delete (§7.2 notes). Archived tags stay
--     visible to historical-report queries but are filtered out of
--     pickers at query time. Their `tag_links` survive for audit.

-- ---------- user_pins ----------------------------------------------

CREATE TABLE dp_user_pins (
    user_id    UUID         NOT NULL REFERENCES dp_users(id) ON DELETE CASCADE,
    kind       TEXT         NOT NULL,
    target_id  UUID         NOT NULL,
    position   INTEGER      NOT NULL,
    pinned_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, kind, target_id),
    CHECK (kind IN ('repo', 'tag'))
);

-- Sidebar render path is `SELECT … WHERE user_id = ? ORDER BY
-- position`. Cover that exactly.
CREATE INDEX dp_user_pins_user_position_idx
    ON dp_user_pins (user_id, position);

-- ---------- tags ----------------------------------------------------

CREATE TABLE dp_tags (
    id              UUID         PRIMARY KEY,
    scope_kind      TEXT         NOT NULL,
    -- Exactly one of these three is non-NULL, matching scope_kind.
    -- Enforced by the CHECK below — gives us real FKs and
    -- ON DELETE CASCADE per scope kind without a polymorphic FK.
    scope_user_id   UUID         NULL REFERENCES dp_users(id) ON DELETE CASCADE,
    scope_team_id   UUID         NULL REFERENCES dp_teams(id) ON DELETE CASCADE,
    scope_org_id    UUID         NULL REFERENCES dp_orgs(id)  ON DELETE CASCADE,
    name            TEXT         NOT NULL,
    color           TEXT         NOT NULL,
    description     TEXT         NULL,
    created_by      UUID         NOT NULL REFERENCES dp_users(id),
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now(),
    archived_at     TIMESTAMPTZ  NULL,
    CHECK (scope_kind IN ('user', 'team', 'org')),
    CHECK (
        (scope_kind = 'user'
            AND scope_user_id IS NOT NULL
            AND scope_team_id IS NULL
            AND scope_org_id  IS NULL)
     OR (scope_kind = 'team'
            AND scope_team_id IS NOT NULL
            AND scope_user_id IS NULL
            AND scope_org_id  IS NULL)
     OR (scope_kind = 'org'
            AND scope_org_id  IS NOT NULL
            AND scope_user_id IS NULL
            AND scope_team_id IS NULL)
    )
);

-- Per-scope case-insensitive name uniqueness (§7.2): two different
-- scopes can both have a tag named "Phoenix"; one scope cannot. The
-- COALESCE collapses the three scope_*_id columns to a single
-- non-NULL UUID (the CHECK above guarantees exactly one is set).
CREATE UNIQUE INDEX dp_tags_scope_name_uniq
    ON dp_tags (
        scope_kind,
        COALESCE(scope_user_id, scope_team_id, scope_org_id),
        lower(name)
    );

-- Pickers / `GET /me/tags` list visible non-archived tags. Cover the
-- common predicate (scope membership + not archived) cheaply.
CREATE INDEX dp_tags_scope_idx
    ON dp_tags (scope_kind, scope_user_id, scope_team_id, scope_org_id)
    WHERE archived_at IS NULL;

-- ---------- tag_links ----------------------------------------------

CREATE TABLE dp_tag_links (
    id              UUID         PRIMARY KEY,
    tag_id          UUID         NOT NULL REFERENCES dp_tags(id) ON DELETE CASCADE,
    kind            TEXT         NOT NULL,
    -- Exactly one target_*_id is non-NULL, matching `kind`. Same
    -- polymorphism-via-CHECK trick as `dp_tags.scope_*_id`.
    target_repo_id  UUID         NULL REFERENCES dp_repos(id)  ON DELETE CASCADE,
    target_issue_id UUID         NULL REFERENCES dp_issues(id) ON DELETE CASCADE,
    target_user_id  UUID         NULL REFERENCES dp_users(id)  ON DELETE CASCADE,
    target_team_id  UUID         NULL REFERENCES dp_teams(id)  ON DELETE CASCADE,
    added_by        UUID         NOT NULL REFERENCES dp_users(id),
    added_at        TIMESTAMPTZ  NOT NULL DEFAULT now(),
    CHECK (kind IN ('repo', 'issue', 'user', 'team')),
    CHECK (
        (kind = 'repo'
            AND target_repo_id  IS NOT NULL
            AND target_issue_id IS NULL
            AND target_user_id  IS NULL
            AND target_team_id  IS NULL)
     OR (kind = 'issue'
            AND target_issue_id IS NOT NULL
            AND target_repo_id  IS NULL
            AND target_user_id  IS NULL
            AND target_team_id  IS NULL)
     OR (kind = 'user'
            AND target_user_id  IS NOT NULL
            AND target_repo_id  IS NULL
            AND target_issue_id IS NULL
            AND target_team_id  IS NULL)
     OR (kind = 'team'
            AND target_team_id  IS NOT NULL
            AND target_repo_id  IS NULL
            AND target_issue_id IS NULL
            AND target_user_id  IS NULL)
    )
);

-- (tag_id, kind, target) is unique — a tag links a given target at
-- most once. COALESCE collapses the four target_*_id columns to a
-- single non-NULL UUID; the CHECK above guarantees exactly one is
-- set.
CREATE UNIQUE INDEX dp_tag_links_tag_target_uniq
    ON dp_tag_links (
        tag_id,
        kind,
        COALESCE(target_repo_id, target_issue_id, target_user_id, target_team_id)
    );

-- Per-target reverse lookup ("what tags link this repo / issue /
-- user / team?"). Four partial indexes — one per kind — instead of
-- one wide index, because each query hits exactly one kind and the
-- partial indexes are tiny.
CREATE INDEX dp_tag_links_repo_idx
    ON dp_tag_links (target_repo_id)  WHERE kind = 'repo';
CREATE INDEX dp_tag_links_issue_idx
    ON dp_tag_links (target_issue_id) WHERE kind = 'issue';
CREATE INDEX dp_tag_links_user_idx
    ON dp_tag_links (target_user_id)  WHERE kind = 'user';
CREATE INDEX dp_tag_links_team_idx
    ON dp_tag_links (target_team_id)  WHERE kind = 'team';
