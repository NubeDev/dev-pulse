/**
 * Canonical list of `EventKind` variants, mirrored from
 * `crates/dp-domain/src/event.rs` (snake_case on the wire, which is
 * what the `activity_types=...` query param expects).
 *
 * The user-report table renders one row per kind, so we keep the
 * order locked here rather than relying on object-key iteration.
 */
export interface ActivityKind {
  readonly key: string;
  readonly label: string;
}

export const ACTIVITY_KINDS: readonly ActivityKind[] = [
  { key: "commit", label: "Commits" },
  { key: "pull_request_opened", label: "PRs opened" },
  { key: "pull_request_merged", label: "PRs merged" },
  { key: "pull_request_closed", label: "PRs closed (unmerged)" },
  { key: "review", label: "Reviews" },
  { key: "review_comment", label: "Review comments" },
  { key: "issue_opened", label: "Issues opened" },
  { key: "issue_closed", label: "Issues closed" },
  { key: "issue_comment", label: "Issue comments" },
  { key: "workflow_run", label: "Workflow runs" },
  { key: "deployment", label: "Deployments" },
  { key: "release", label: "Releases" },
];
