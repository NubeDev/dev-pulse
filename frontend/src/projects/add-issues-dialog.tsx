/**
 * §6.3 / PROJECT-VIEW.md §5.4 Add-issue dialog — the single
 * detail-page entry point for getting issues into a project.
 *
 * Two tabs:
 *   - **Add existing** — substring search over
 *     `GET /issues?org_id=<project.org_id>&q=…` plus multi-select
 *     and bulk POST `/projects/{id}/issues`. Per-row outcomes
 *     (`added` / `skipped` with reason) are surfaced inline.
 *   - **Create new** — title + body + repo picker against
 *     `POST /issues`. The backend creates on GitHub, materialises
 *     the local `dp_issues` row from the GitHub payload
 *     synchronously, and attaches it to the project / view in
 *     the same request.
 *
 * Tab-aware: when `activeViewId` is set the dialog title shows
 * the view name and both panes attach to that view's membership
 * table only (no project-level mutation). When `activeViewId` is
 * null the All-tab project-level semantics apply (CAS-gated on
 * `project.version`).
 */

import { useEffect, useState } from "react";
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
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";

import {
  api,
  BULK_ADD_ISSUE_CAP,
  type BulkAddResult,
  type IssueListItem,
  type IssueListResponse,
  type ProjectDto,
  type ProjectRepoDto,
} from "../api/client.js";

import { categoryTagName } from "./view-wizard/index.js";

import {
  useAddIssuesToProject,
  useCreateIssue,
  useProjectRepos,
  useProjectViews,
} from "./use-projects-data.js";

export interface AddIssuesDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  project: ProjectDto;
  /** PROJECT-VIEW.md §5.4 amendment — when the user opened the
   *  dialog from a named saved-view tab, accepted issues are
   *  attached to that view's membership only. `null` / omitted =
   *  the "All" tab, project-level. */
  activeViewId?: string | null;
  /** Optional active-view display name to title the dialog.
   *  When omitted (All tab) the title falls back to the project
   *  name. */
  activeViewName?: string | null;
  /** PROJECT-VIEW.md category amendment — when the active view
   *  is a category view (single `tag:category:<key>` filter),
   *  the create-new flow auto-tags the new issue with the
   *  matching org tag so users don't have to remember. */
  activeCategoryKey?: string | null;
  /** Tag id resolved from [`activeCategoryKey`] at the project's
   *  org. `null` when the tag hasn't been loaded yet or doesn't
   *  exist — in either case the auto-tag step is skipped. */
  activeCategoryTagId?: string | null;
}

export function AddIssuesDialog({
  open,
  onOpenChange,
  project,
  activeViewId,
  activeViewName,
  activeCategoryKey,
  activeCategoryTagId,
}: AddIssuesDialogProps): JSX.Element {
  const [tab, setTab] = useState<"existing" | "new">("existing");
  // The destination tab the issue(s) will be attached to. `null`
  // = "All" (project-level membership, CAS-gated on
  // `project.version`). Any other value = the named saved-view's
  // membership table only (no project-level mutation). Seeded
  // from `activeViewId` so opening the dialog from a view tab
  // pre-selects that tab, and the user can change destination
  // from the picker without closing.
  const [destinationViewId, setDestinationViewId] = useState<string | null>(
    activeViewId ?? null,
  );

  // Available saved views for the destination picker. Empty list
  // collapses the picker entirely (nothing to pick between).
  const viewsQ = useProjectViews(project.id);
  const views = viewsQ.data ?? [];

  // Reset to the default tab + destination on every reopen so
  // users don't land on stale selections from a prior session.
  useEffect(() => {
    if (open) {
      setTab("existing");
      setDestinationViewId(activeViewId ?? null);
    }
  }, [open, activeViewId]);

  const close = (): void => onOpenChange(false);

  // Resolve the chosen destination's display name from the live
  // views list (so renames inside the same session are reflected)
  // and fall back to `activeViewName` only when the views query
  // hasn't completed yet.
  const selectedView = destinationViewId
    ? views.find((v) => v.id === destinationViewId)
    : undefined;
  const destinationLabel = destinationViewId
    ? `“${selectedView?.name ?? activeViewName ?? "this tab"}”`
    : project.name;

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
        data-testid="add-issue-dialog"
      >
        <DialogHeader>
          <DialogTitle>Add issue to {destinationLabel}</DialogTitle>
          <DialogDescription>
            {destinationViewId
              ? "The issue will appear in this tab only."
              : "The issue will appear in this project."}
          </DialogDescription>
        </DialogHeader>

        {views.length > 0 && (
          <div className="flex items-center gap-2">
            <Label
              htmlFor="add-issue-destination"
              className="text-xs text-muted-foreground"
            >
              Destination
            </Label>
            <Select
              value={destinationViewId ?? "__all__"}
              onValueChange={(v) =>
                setDestinationViewId(v === "__all__" ? null : v)
              }
            >
              <SelectTrigger
                id="add-issue-destination"
                className="w-full"
                data-testid="add-issue-destination"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__all__">All (project)</SelectItem>
                {views.map((v) => (
                  <SelectItem key={v.id} value={v.id}>
                    {v.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}

        <Tabs
          value={tab}
          onValueChange={(v) => setTab(v as "existing" | "new")}
          className="min-h-0 flex-1"
        >
          <TabsList>
            <TabsTrigger value="existing" data-testid="add-issue-tab-existing">
              Add existing
            </TabsTrigger>
            <TabsTrigger value="new" data-testid="add-issue-tab-new">
              Create new
            </TabsTrigger>
          </TabsList>
          <TabsContent
            value="existing"
            className="min-h-0 flex-1 flex-col gap-3 data-[state=active]:flex"
          >
            <ExistingPanel
              project={project}
              activeViewId={destinationViewId}
              onClose={close}
            />
          </TabsContent>
          <TabsContent
            value="new"
            className="min-h-0 flex-1 flex-col gap-3 data-[state=active]:flex"
          >
            <NewPanel
              project={project}
              activeViewId={destinationViewId}
              activeCategoryKey={activeCategoryKey ?? null}
              activeCategoryTagId={activeCategoryTagId ?? null}
              onClose={close}
            />
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

// ---------------------------------------------------------------------------
// Existing — substring search + multi-select + bulk attach.
// ---------------------------------------------------------------------------

function ExistingPanel({
  project,
  activeViewId,
  onClose,
}: {
  project: ProjectDto;
  activeViewId: string | null;
  onClose: () => void;
}): JSX.Element {
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [result, setResult] = useState<BulkAddResult | null>(null);
  const [linkedReposOnly, setLinkedReposOnly] = useState(true);

  const projectRepos = useProjectRepos(project.id);
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
    enabled: !linkedReposOnly || !projectRepos.isPending,
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
        // Backend ignores `expected_version` when `view_id` is set;
        // supplying it is harmless and keeps the All-tab path CAS-
        // correct.
        expected_version: project.version,
        issue_ids: Array.from(selected),
        view_id: activeViewId ?? undefined,
      },
      {
        onSuccess: (r) => {
          setResult(r);
          setSelected(new Set());
        },
      },
    );
  };

  return (
    <>
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
          onClick={onClose}
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
    </>
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
      {row.is_local ? (
        <Badge
          variant="outline"
          className="shrink-0 border-amber-500/60 px-1.5 py-0 text-[10px] uppercase text-amber-700 dark:text-amber-300"
          title="Local-only note (not synced to GitHub)"
        >
          local
        </Badge>
      ) : (
        <span className="shrink-0 font-mono text-xs text-muted-foreground tabular-nums">
          {row.repo_slug ?? "—"}#{row.number}
        </span>
      )}
      <span className="flex-1 truncate">{row.title}</span>
    </label>
  );
}

// ---------------------------------------------------------------------------
// New — create on GitHub and attach in one request.
// ---------------------------------------------------------------------------

function NewPanel({
  project,
  activeViewId,
  activeCategoryKey,
  activeCategoryTagId,
  onClose,
}: {
  project: ProjectDto;
  activeViewId: string | null;
  activeCategoryKey: string | null;
  activeCategoryTagId: string | null;
  onClose: () => void;
}): JSX.Element {
  const repoLinks = useProjectRepos(project.id);
  const repos: ProjectRepoDto[] = repoLinks.data ?? [];
  const create = useCreateIssue(project.id);

  const [repoId, setRepoId] = useState("");
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [tagError, setTagError] = useState<string | null>(null);

  // Seed / refresh the repo selection when the list loads.
  useEffect(() => {
    setRepoId((prev) => {
      if (prev && repos.some((r) => r.repo_id === prev)) return prev;
      return repos[0]?.repo_id ?? "";
    });
  }, [repos]);

  const canSubmit =
    !create.isPending &&
    !create.isSuccess &&
    repoId.length > 0 &&
    title.trim().length > 0;

  // Two-button submit: `local` short-circuits the GitHub POST on
  // the backend. The repo picker still applies (the issue is
  // org/repo-scoped for permissions and project membership) but
  // the row carries `is_local = true` and the table hides the
  // repo badge for it.
  //
  // Category auto-tag: when the user opened the dialog from a
  // category tab, we (a) pass the matching `category:<key>` as a
  // GitHub label so the GH issue carries it natively (non-local
  // only — local issues never touch GH), and (b) once the
  // backend has materialised the local row, link the DP tag so
  // the category chip shows up on the workbench immediately.
  const submit = (local: boolean): void => {
    if (!canSubmit) return;
    setTagError(null);
    const categoryLabel =
      !local && activeCategoryKey ? categoryTagName(activeCategoryKey) : null;
    create.mutate(
      {
        repo_id: repoId,
        title: title.trim(),
        body: body.trim() ? body.trim() : undefined,
        labels: categoryLabel ? [categoryLabel] : undefined,
        project_id: project.id,
        view_id: activeViewId ?? undefined,
        expected_version: activeViewId ? undefined : project.version,
        local,
      },
      {
        onSuccess: async (resp) => {
          if (!activeCategoryTagId || !resp.issue_id) return;
          try {
            await api.linkTagTargets(activeCategoryTagId, {
              items: [{ kind: "issue", target_id: resp.issue_id }],
            });
          } catch (err) {
            setTagError(
              err instanceof Error ? err.message : String(err),
            );
          }
        },
      },
    );
  };

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    // Default form submit (Enter key in the title field) = local
    // create. The "Create and sync to GitHub" button is an
    // explicit opt-in.
    submit(true);
  };

  if (create.isSuccess) {
    return (
      <div
        className="flex flex-col gap-3"
        data-testid="add-issue-new-success"
      >
        <Alert>
          <AlertTitle>Created #{create.data.number}</AlertTitle>
          <AlertDescription>
            {create.data.issue_id
              ? activeViewId
                ? "The issue is live on GitHub and attached to this tab."
                : "The issue is live on GitHub and attached to this project."
              : "The issue is live on GitHub. It'll appear in the list within a few seconds once sync catches up."}
          </AlertDescription>
        </Alert>
        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={() => {
              create.reset();
              setTitle("");
              setBody("");
              setTagError(null);
            }}
          >
            Create another
          </Button>
          <Button type="button" onClick={onClose}>
            Done
          </Button>
        </DialogFooter>
      </div>
    );
  }

  return (
    <form className="flex flex-col gap-4" onSubmit={onSubmit}>
      {activeCategoryKey && (
        <div
          className="rounded-md border border-primary/30 bg-primary/5 px-3 py-2 text-xs text-foreground"
          data-testid="add-issue-new-category-notice"
        >
          New issues will be tagged{" "}
          <code className="font-mono">
            {categoryTagName(activeCategoryKey)}
          </code>
          {activeCategoryTagId ? (
            <> so they land in this category tab automatically.</>
          ) : (
            <>
              {" "}— the matching tag will appear once it syncs to the
              project's org.
            </>
          )}
        </div>
      )}

      <div className="flex flex-col gap-2">
        <Label htmlFor="add-issue-new-repo">Repo</Label>
        {repos.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No repos linked to this project yet. Open Settings → Manage
            repos… to link one.
          </p>
        ) : (
          <Select value={repoId} onValueChange={setRepoId}>
            <SelectTrigger
              id="add-issue-new-repo"
              data-testid="add-issue-new-repo"
            >
              <SelectValue placeholder="Select a repo" />
            </SelectTrigger>
            <SelectContent>
              {repos.map((r) => (
                <SelectItem key={r.repo_id} value={r.repo_id}>
                  {r.repo_name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}
      </div>

      <div className="flex flex-col gap-2">
        <Label htmlFor="add-issue-new-title">Title</Label>
        <Input
          id="add-issue-new-title"
          data-testid="add-issue-new-title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="Short, action-oriented summary"
          maxLength={300}
          autoFocus
          required
        />
      </div>

      <div className="flex flex-col gap-2">
        <Label htmlFor="add-issue-new-body">Body (Markdown)</Label>
        <Textarea
          id="add-issue-new-body"
          data-testid="add-issue-new-body"
          rows={6}
          value={body}
          onChange={(e) => setBody(e.target.value)}
          placeholder="What's the context? What does done look like?"
        />
      </div>

      {create.isError && (
        <Alert variant="destructive" data-testid="add-issue-new-error">
          <AlertTitle>Create failed</AlertTitle>
          <AlertDescription>{create.error.message}</AlertDescription>
        </Alert>
      )}

      {tagError && (
        <Alert
          variant="destructive"
          data-testid="add-issue-new-tag-error"
        >
          <AlertTitle>Issue created, but tagging failed</AlertTitle>
          <AlertDescription>{tagError}</AlertDescription>
        </Alert>
      )}

      <DialogFooter>
        <Button
          type="button"
          variant="ghost"
          onClick={onClose}
          disabled={create.isPending}
        >
          Cancel
        </Button>
        <Button
          type="button"
          variant="secondary"
          data-testid="add-issue-new-submit-local"
          onClick={() => submit(true)}
          disabled={!canSubmit}
          title="Create as a local-only note (not pushed to GitHub)"
        >
          {create.isPending ? "Creating…" : "Create"}
        </Button>
        <Button
          type="button"
          data-testid="add-issue-new-submit"
          onClick={() => submit(false)}
          disabled={!canSubmit}
          title="Create on GitHub and mirror locally"
        >
          {create.isPending ? "Creating…" : "Create and sync to GitHub"}
        </Button>
      </DialogFooter>
    </form>
  );
}
