-- 0021_issue_github_node_id.sql  (linear-projects-idea.md §3.10)
--
-- Adds `github_node_id` to `dp_issues` — the opaque GitHub
-- GraphQL node id (e.g. `I_kwDOABC...`) that the Projects v2
-- mirror needs as the `contentId` argument to
-- `addProjectV2ItemById`. Without it, the §3.10 mirror has to
-- resolve the id lazily on every first-mirror call via
-- `repository(owner, name) { issue(number) { id } }` — an extra
-- round-trip on every fresh row.
--
-- Nullable so old rows pre-dating this column resolve lazily on
-- their first mirror attempt and get the column populated
-- opportunistically. New webhook deliveries and backfill rows
-- capture the value off `issue.node_id` (always present on the
-- GitHub payload) so the lazy path is exercised only for rows
-- ingested before this migration shipped.
--
-- Not indexed — we never query *by* node id from dev-pulse; the
-- column is read-only after first write and surfaces only to the
-- mirror adapter. Adding an index would carry a write cost on
-- every issue upsert with no read to pay for it.

ALTER TABLE dp_issues
    ADD COLUMN github_node_id TEXT NULL;
