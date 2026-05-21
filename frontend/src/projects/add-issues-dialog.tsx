/**
 * §6.3 Add-issues dialog — the project-detail surface for
 * attaching issues to a project. v1 ships a substring search over
 * `GET /issues?org_id=<project.org_id>&q=…`; the user multi-selects
 * rows and submits them as a single `POST /projects/{id}/issues`
 * (CAS-gated on `project.version`).
 *
 * Per-row outcomes from the `BulkAddResult` envelope are surfaced
 * inline so the user can see exactly which issues were added and
 * which were skipped (with reason) — the §7.2 contract is
 * specifically optimised for this: one round-trip, per-row
 * verdicts, no second probe.
 *
 * v1 limitations carried forward from `SCOPE-PROJECTS.md`:
 *   - search is by title substring only (no advanced filters);
 *   - the triage detail-pane / bulk-add-from-list surfaces (§6.5
 *     / §6.6) are separate components; this dialog is the
 *     detail-page entry point only.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";

import {
  api,
  BULK_ADD_ISSUE_CAP,
  type BulkAddResult,
  type IssueListItem,
  type IssueListResponse,
  type ProjectDto,
} from "../api/client.js";

import { useAddIssuesToProject, useProjectRepos } from "./use-projects-data.js";

export interface AddIssuesDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  project: ProjectDto;
}

export function AddIssuesDialog({
  open,
  onOpenChange,
  project,
}: AddIssuesDialogProps): JSX.Element {
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [result, setResult] = useState<BulkAddResult | null>(null);
  const [linkedReposOnly, setLinkedReposOnly] = useState(true);

  const projectRepos = useProjectRepos(open ? project.id : null);
  const repoIds = (projectRepos.data ?? []).map((r) => r.repo_id);
  const filterByRepos = linkedReposOnly && repoIds.length > 0;

  const issuesQ = useQuery<IssueListResponse>({
    queryKey: [
      "issues",
      "for-project-add",
      project.org_id,
      search.trim(),
      filterByRepos ? repoIds.slice().sort().join(",") : "",
    ],
    queryFn: () =>
      api.listIssues({
        org_id: project.org_id,
        state: "all",
        q: search.trim() || undefined,
        repo_ids: filterByRepos ? repoIds : undefined,
        limit: 50,
      }),
    enabled: open && (!linkedReposOnly || !projectRepos.isPending),
    staleTime: 10_000,
  });

  const add = useAddIssuesToProject(project.id);

  const rows = issuesQ.data?.rows ?? [];

  const toggle = (id: string): void => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else if (next.size < BULK_ADD_ISSUE_CAP) next.add(id);
      return next;
    });
  };

  const onSubmit = (): void => {
    if (selected.size === 0) return;
    add.mutate(
      {
        expected_version: project.version,
        issue_ids: Array.from(selected),
      },
      {
        onSuccess: (r) => {
          setResult(r);
          setSelected(new Set());
        },
      },
    );
  };

  const close = (): void => {
    setSearch("");
    setSelected(new Set());
    setResult(null);
    add.reset();
    onOpenChange(false);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) close();
        else onOpenChange(o);
      }}
    >
      <DialogContent
        className="sm:max-w-2xl max-h-[85vh] flex flex-col overflow-hidden"
        data-testid="add-issues-dialog"
      >
        <DialogHeader>
          <DialogTitle>Add issues to {project.name}</DialogTitle>
          <DialogDescription>
            Pick up to {BULK_ADD_ISSUE_CAP} issues from this org. Issues already
            attached to a different project are skipped with a clear reason.
          </DialogDescription>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-3">
          <Input
            data-testid="add-issues-search"
            placeholder="Search by title…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            autoFocus
          />

          {repoIds.length > 0 && (
            <label className="flex items-center gap-2 text-xs text-muted-foreground">
              <Checkbox
                checked={linkedReposOnly}
                onCheckedChange={(v) => setLinkedReposOnly(!!v)}
                data-testid="add-issues-linked-repos-only"
              />
              Only show issues from this project's {repoIds.length} linked
              repo{repoIds.length === 1 ? "" : "s"}
            </label>
          )}

          <div className="flex items-center justify-between text-xs text-muted-foreground">
            <span data-testid="add-issues-selected-count">
              {selected.size} selected (cap {BULK_ADD_ISSUE_CAP})
            </span>
            {issuesQ.data && (
              <span>
                {rows.length} of {issuesQ.data.total} matching
              </span>
            )}
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto rounded-md border border-border">
            {issuesQ.isPending && (
              <div className="flex items-center gap-2 px-3 py-4 text-sm text-muted-foreground">
                <Spinner /> Loading issues…
              </div>
            )}
            {issuesQ.isError && (
              <Alert variant="destructive" className="m-2">
                <AlertTitle>Couldn't load issues</AlertTitle>
                <AlertDescription>{issuesQ.error.message}</AlertDescription>
              </Alert>
            )}
            {!issuesQ.isPending && !issuesQ.isError && rows.length === 0 && (
              <p className="px-3 py-6 text-center text-sm text-muted-foreground">
                No issues match that search.
              </p>
            )}
            {rows.map((row) => (
              <AddIssueRow
                key={row.id}
                row={row}
                checked={selected.has(row.id)}
                onToggle={() => toggle(row.id)}
              />
            ))}
          </div>

          {add.error && (
            <Alert variant="destructive" data-testid="add-issues-error">
              <AlertTitle>Add failed</AlertTitle>
              <AlertDescription>{add.error.message}</AlertDescription>
            </Alert>
          )}

          {result && (
            <Alert data-testid="add-issues-result">
              <AlertTitle>
                Added {result.added.length}, skipped {result.skipped.length}
              </AlertTitle>
              <AlertDescription>
                {result.skipped.length > 0 && (
                  <ul className="mt-2 list-disc pl-5 text-xs">
                    {result.skipped.map((s) => (
                      <li key={s.issue_id}>
                        <code className="font-mono">{s.issue_id.slice(0, 8)}</code>
                        : {s.reason}
                      </li>
                    ))}
                  </ul>
                )}
              </AlertDescription>
            </Alert>
          )}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={close}
            disabled={add.isPending}
          >
            Close
          </Button>
          <Button
            type="button"
            data-testid="add-issues-submit"
            onClick={onSubmit}
            disabled={selected.size === 0 || add.isPending}
          >
            {add.isPending
              ? "Adding…"
              : `Add ${selected.size} issue${selected.size === 1 ? "" : "s"}`}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function AddIssueRow({
  row,
  checked,
  onToggle,
}: {
  row: IssueListItem;
  checked: boolean;
  onToggle: () => void;
}): JSX.Element {
  return (
    <label
      className="flex cursor-pointer items-center gap-3 border-b border-border/60 px-3 py-2 text-sm last:border-b-0 hover:bg-accent/30"
      data-testid="add-issues-row"
    >
      <Checkbox checked={checked} onCheckedChange={onToggle} />
      <Badge
        variant={row.state === "open" ? "default" : "secondary"}
        className="shrink-0 px-1.5 py-0 text-[10px] uppercase"
      >
        {row.state}
      </Badge>
      <span className="shrink-0 font-mono text-xs text-muted-foreground tabular-nums">
        {row.repo_slug ?? "—"}#{row.number}
      </span>
      <span className="flex-1 truncate">{row.title}</span>
    </label>
  );
}
