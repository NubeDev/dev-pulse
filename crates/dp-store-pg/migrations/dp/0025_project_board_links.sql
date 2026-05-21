-- 0025_project_board_links.sql  (linear-projects-v2.md §5, §8 — slice B)
--
-- Project → GitHub Projects v2 board mirror plumbing. Two tables
-- land here; the legacy per-repo `dp_repo_project_link` table is
-- physically dropped by `0026_drop_repo_project_link.sql` once this
-- migration is in.
--
--   * `dp_project_board_links` — one row per (project, GitHub board)
--     mirror target. A project can link **many** boards (e.g. an
--     Eng Sprint board AND a cross-team Roadmap board); mirror writes
--     fan out across all of them. The cached `github_board_title` /
--     `github_board_url` / `github_board_cached_at` are refreshed
--     opportunistically by the §7.3 picker and by the nightly safety-
--     net job, so renamed boards surface within 24h instead of
--     waiting on a user-visible read.
--
--   * `dp_project_board_items` — per (link, issue) projection state.
--     Each issue is projected to a **distinct** GitHub Projects v2
--     item id per board (`PVTI_…`); a single column on
--     `dp_issue_dates` cannot represent N items, which is why the
--     §3.10-era `dp_issue_dates.mirror_node_id` column is no longer
--     authoritative. Subsequent PATCH /issues/{id}/dates calls look
--     up the `(link_id, issue_id)` row to find the item to update.
--
-- NB: the §3.10-era `dp_issue_dates.mirror_node_id /
-- mirror_synced_at / mirror_error` columns are kept in place by
-- this migration on purpose. The §3.10 mirror still owns
-- `PATCH /issues/{id}/dates`; the rewire to fan out across this
-- table lands in a later slice-B stage together with the issue-dates
-- DTO reshape (§7.4 `207 Multi-Status` response) and the mirror
-- adapter rewrite. Dropping the columns ahead of that rewrite would
-- strand the live mirror with no place to record its outcome.

-- ---------- dp_project_board_links ---------------------------------

-- One row per (project, GitHub board). The natural key is
-- `(project_id, github_board_node_id)` — a project never links the
-- same physical board twice, but absolutely can link several
-- distinct boards. The surrogate `id` exists so the §7.3 DELETE
-- handler can take an opaque link id rather than the GitHub node id
-- (which is awkward in URLs and a leak of the GitHub schema).
--
-- `github_board_title` / `github_board_url` are *cached* display
-- fields, refreshed by the picker. They're nullable here because the
-- picker may not have run yet at write time on the link-now /
-- backfill-later code path; the §7.3 GET endpoint always returns the
-- freshest values it has and a background job (§6.4) re-refreshes
-- once a day so renames don't silently rot.
--
-- `*_field_node_id` columns are nullable because not every board
-- defines a Start, Due, or Status field. The mirror skips the lane
-- whenever the column is NULL (§7.4 inheritance from §3.10).
-- `status_field_node_id` is reserved — the mirror does not write to
-- it in v1 (§5 entity table), but the schema carries it so a
-- v2 expansion can land without another migration.
--
-- `last_mirror_at` / `last_mirror_error` are the **aggregate**
-- per-link status — the most recent (success, error) pair the
-- mirror produced across any item under this link. Per-item status
-- lives on `dp_project_board_items`; the aggregate exists so the
-- §6.4 link row in the project detail UI can render
-- `Last sync: 14:23:07 ✓` without a per-issue scan.
CREATE TABLE dp_project_board_links (
    id                       UUID         NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id               UUID         NOT NULL REFERENCES dp_projects(id) ON DELETE CASCADE,
    github_board_node_id     TEXT         NOT NULL,
    github_board_title       TEXT         NULL,
    github_board_url         TEXT         NULL,
    github_board_cached_at   TIMESTAMPTZ  NULL,
    start_field_node_id      TEXT         NULL,
    due_field_node_id        TEXT         NULL,
    status_field_node_id     TEXT         NULL,
    last_mirror_at           TIMESTAMPTZ  NULL,
    last_mirror_error        TEXT         NULL,
    created_by               UUID         NULL REFERENCES dp_users(id) ON DELETE SET NULL,
    created_at               TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ  NOT NULL DEFAULT now(),
    UNIQUE (project_id, github_board_node_id)
);

-- Mirror fan-out lookup: §7.4 step 1 is "for each linked board…",
-- which is a `WHERE project_id = $1` scan. The UNIQUE constraint
-- above already covers `(project_id, github_board_node_id)`; this
-- standalone index keeps the project-only lookup cheap when a
-- project carries no boards yet and the optimizer would otherwise
-- consider a full scan.
CREATE INDEX dp_project_board_links_project_idx
    ON dp_project_board_links (project_id);

-- ---------- dp_project_board_items ---------------------------------

-- Per (link, issue) projection state. PK on the composite so a
-- mirror retry against an already-projected pair is an UPSERT, not a
-- duplicate row. `item_node_id` is the GitHub Projects v2 *item*
-- node id (`PVTI_…`) returned by `addProjectV2ItemById` the first
-- time we mirror this pair; subsequent edits target the same id via
-- `updateProjectV2ItemFieldValue`.
--
-- `last_synced_at` / `last_error` carry the most recent outcome.
-- On success the worker clears `last_error` so the UI doesn't keep
-- showing a stale failure after the operator fixed it.
--
-- Cascading deletes on both FKs so dropping a link or an issue
-- cleans up the projection rows without orphans.
CREATE TABLE dp_project_board_items (
    link_id          UUID         NOT NULL REFERENCES dp_project_board_links(id) ON DELETE CASCADE,
    issue_id         UUID         NOT NULL REFERENCES dp_issues(id)              ON DELETE CASCADE,
    item_node_id     TEXT         NOT NULL,
    last_synced_at   TIMESTAMPTZ  NULL,
    last_error       TEXT         NULL,
    created_at       TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ  NOT NULL DEFAULT now(),
    PRIMARY KEY (link_id, issue_id)
);

-- Reverse lookup: "which boards is this issue projected to?" is the
-- §6.5 detail-pane `SyncStatus` aggregate, which renders one row per
-- (link, issue) outcome under the issue. The PK above already
-- prefixes on `link_id`; this index gives the issue-scoped scan an
-- equally cheap path so the aggregate UI doesn't do a sequential
-- scan when the table grows.
CREATE INDEX dp_project_board_items_issue_idx
    ON dp_project_board_items (issue_id);
