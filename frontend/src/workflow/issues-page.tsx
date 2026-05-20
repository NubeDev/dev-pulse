/**
 * Issues page — SCOPE-PROJECTS §8.
 *
 * Two surfaces share this file:
 *
 * 1. **Create issue** form — `POST /issues`. No `expected_version`
 *    (there is no row to CAS yet). Gated behind the §8.4
 *    `WritesGate` for the selected org.
 *
 * 2. **Edit issue** form — `PATCH /issues/{id}` (and `POST
 *    /issues/{id}/comments`). The CAS-on-version path: the form
 *    loads the issue, captures `version` as `expected_version`, and
 *    on submit either succeeds or surfaces the §8.3 stale-version
 *    reload UX:
 *
 *      "This issue changed since you opened the form. Reload to see
 *      the new state, then re-apply your edit."
 *
 *    The reload re-runs the GET, drops the cached form state, and
 *    re-prompts. The §8.3 contract guarantees the server hands us
 *    `current_version` on the 409 so the reload is single-shot —
 *    we don't need a second GET to learn the new version, just the
 *    new field values.
 *
 * Issue-management UI is intentionally minimal in this stage —
 * everything that matters for "the §8 write path lands in the
 * frontend" is wired here, but per-org repo pickers and a real
 * issue listing view are deferred to a follow-up.
 */

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  IconAlertTriangle,
  IconCheck,
  IconClock,
  IconExternalLink,
  IconInbox,
  IconKeyboard,
  IconRefresh,
  IconX,
} from "@tabler/icons-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../components/table.jsx";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";

import { api, type IssueDto, type ListIssuesQuery } from "../api/client.js";
import { Markdown } from "../components/markdown.jsx";
import { PageHeading } from "../components/page-heading.jsx";
import {
  navigate,
  useRoute,
  workflowIssuesRoute,
  workflowSelectedIssue,
  workflowSelectedRepoId,
} from "../routes.js";

import { USE_MOCK, mockAppInstallBanner, mockIssue, mockRepoList } from "./mocks.js";
import {
  staleVersionFromError,
  useCommentOnIssue,
  useIssue,
  useIssueDates,
  useIssueList,
  useMarkInboxSeen,
  useMyQueue,
  useSetInboxState,
  useUpdateIssue,
  useUpdateIssueDates,
  writesUnavailableOrg,
} from "./use-workflow-data.js";
import { WritesGate } from "./writes-banner.js";

const PAGE_SIZE = 50;

/** Per-page URL query state derived from the hash route. */
interface IssuesPageQuery {
  repoId: string | null;
  state: "open" | "closed" | "all";
  assignee: string;
  q: string;
  offset: number;
  /** Triage view (`linear-projects-idea.md` §3.5):
   *  - `list`     — the original "every issue" pane.
   *  - `inbox`    — `GET /me/queue`; default landing view.
   *  - `untriaged`— `?untriaged=true`; rows with no assignee /
   *                  label. */
  view: "list" | "inbox" | "untriaged";
}

function parseQuery(route: string): IssuesPageQuery {
  const qIdx = route.indexOf("?");
  const params = qIdx >= 0 ? new URLSearchParams(route.slice(qIdx + 1)) : new URLSearchParams();
  const stateParam = params.get("state");
  const state: "open" | "closed" | "all" =
    stateParam === "closed" || stateParam === "all" ? stateParam : "open";
  const offsetRaw = Number.parseInt(params.get("offset") ?? "0", 10);
  const viewParam = params.get("view");
  const view: IssuesPageQuery["view"] =
    viewParam === "inbox" || viewParam === "untriaged" ? viewParam : "list";
  return {
    repoId: params.get("repo_id"),
    state,
    assignee: params.get("assignee") ?? "",
    q: params.get("q") ?? "",
    offset: Number.isFinite(offsetRaw) && offsetRaw > 0 ? offsetRaw : 0,
    view,
  };
}

function buildRoute(q: IssuesPageQuery, issueId: string | null): string {
  const params = new URLSearchParams();
  if (q.view !== "list") params.set("view", q.view);
  if (q.repoId) params.set("repo_id", q.repoId);
  if (q.state !== "open") params.set("state", q.state);
  if (q.assignee) params.set("assignee", q.assignee);
  if (q.q) params.set("q", q.q);
  if (q.offset > 0) params.set("offset", String(q.offset));
  if (issueId) params.set("issue", issueId);
  const qs = params.toString();
  return qs ? `#/workflow/issues?${qs}` : "#/workflow/issues";
}

/** Lightweight cached lookup so the filter chip can render the
 *  repo slug without round-tripping `GET /repos` for each filter. */
function useRepoLookup(repoId: string | null) {
  return useQuery({
    queryKey: ["workflow", "repo-lookup", repoId],
    enabled: !!repoId,
    staleTime: 5 * 60_000,
    queryFn: async () => {
      if (!repoId) return null;
      if (USE_MOCK) return mockRepoList.find((r) => r.id === repoId) ?? null;
      const res = await api.listRepos({ limit: 200 });
      return res.rows.find((r) => r.id === repoId) ?? null;
    },
  });
}

export function IssuesPage(): JSX.Element {
  const route = useRoute();
  const parsed = useMemo(() => parseQuery(route), [route]);
  const selectedIssueId = workflowSelectedIssue(route);

  // Local input state so the search box doesn't refire on every
  // keystroke. Sync back to the URL on blur / Enter.
  const [searchDraft, setSearchDraft] = useState(parsed.q);
  const [assigneeDraft, setAssigneeDraft] = useState(parsed.assignee);
  useEffect(() => setSearchDraft(parsed.q), [parsed.q]);
  useEffect(() => setAssigneeDraft(parsed.assignee), [parsed.assignee]);

  const repoLookup = useRepoLookup(parsed.repoId);

  const query: ListIssuesQuery = useMemo(
    () => ({
      repo_id: parsed.repoId ?? undefined,
      state: parsed.state,
      assignee: parsed.assignee || undefined,
      q: parsed.q || undefined,
      // `linear-projects-idea.md` §3.5 — the "Untriaged" smart view
      // pre-narrows the same /me/queue (or /issues) endpoint.
      untriaged: parsed.view === "untriaged" ? true : undefined,
      limit: PAGE_SIZE,
      offset: parsed.offset,
    }),
    [parsed],
  );
  // Branch the data source on the active view. The inbox view hits
  // `GET /me/queue`, which adds the §3.8 visibility predicates
  // (`status <> 'done'`, snooze wake-up) and the `unread` flag.
  const listResult = useIssueList(query);
  const queueResult = useMyQueue(query);
  const issues = parsed.view === "inbox" ? queueResult : listResult;

  const markSeen = useMarkInboxSeen();
  const setInboxState = useSetInboxState();

  const goTo = (next: Partial<IssuesPageQuery>): void => {
    navigate(buildRoute({ ...parsed, ...next, offset: 0 }, selectedIssueId));
  };
  const goToOffset = (offset: number): void => {
    navigate(buildRoute({ ...parsed, offset }, selectedIssueId));
  };
  const openIssue = (id: string | null): void => {
    // §3.8 — opening a row in the peek panel clears the unread dot
    // for that row. Best-effort: the mutation swallows errors so a
    // network blip never blocks the open.
    if (id) markSeen.mutate([id]);
    navigate(buildRoute(parsed, id));
  };

  const total = issues.data?.total ?? 0;
  const rows = issues.data?.rows ?? [];
  const firstShown = total === 0 ? 0 : parsed.offset + 1;
  const lastShown = Math.min(parsed.offset + rows.length, total);

  // ----- Keyboard nav (linear-projects-idea.md §3.7) ---------------------
  //
  // Bindings (only when no input is focused and the peek sheet is
  // closed, except for Esc which always closes the peek):
  //
  //   j / k    move cursor down / up one row
  //   Enter    open the cursor row in the peek panel
  //   Esc      close the peek panel
  //   e        mark cursor row done
  //   h        snooze cursor row 1 day
  //   ?        toggle the help overlay
  //
  // Cursor is a row index into the current page; it clamps when
  // the row set changes (filter / page) and scrolls the highlighted
  // row into view via `rowRefs`.
  const [cursor, setCursor] = useState(0);
  const [helpOpen, setHelpOpen] = useState(false);

  useEffect(() => {
    setCursor((c) => (rows.length === 0 ? 0 : Math.min(c, rows.length - 1)));
  }, [rows.length]);

  useEffect(() => {
    // `TableRow` doesn't forward refs; pin the cursor row by data
    // attribute so the scroll-into-view stays cheap.
    const el = document.querySelector<HTMLTableRowElement>(
      '[data-testid="issues-table"] tr[data-cursor="true"]',
    );
    el?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  useEffect(() => {
    const inEditable = (el: EventTarget | null): boolean => {
      const n = el as HTMLElement | null;
      if (!n) return false;
      const tag = n.tagName;
      return (
        tag === "INPUT" ||
        tag === "TEXTAREA" ||
        tag === "SELECT" ||
        n.isContentEditable
      );
    };
    const snoozeOneDay = (id: string): void => {
      const wake = new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString();
      setInboxState.mutate({ issueId: id, status: "snoozed", snoozed_until: wake });
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      // `?` and Esc work even while typing, with caveats.
      if (e.key === "?" && !inEditable(e.target)) {
        setHelpOpen((h) => !h);
        e.preventDefault();
        return;
      }
      if (e.key === "Escape") {
        if (helpOpen) {
          setHelpOpen(false);
          e.preventDefault();
          return;
        }
        if (selectedIssueId) {
          openIssue(null);
          e.preventDefault();
          return;
        }
      }
      if (inEditable(e.target)) return;
      // The peek panel owns the keyboard while open so the form
      // can keep typing-shortcuts (Tab, Shift+Tab, etc.).
      if (selectedIssueId) return;
      if (helpOpen) return;
      if (rows.length === 0) return;
      switch (e.key) {
        case "j":
        case "ArrowDown":
          setCursor((c) => Math.min(rows.length - 1, c + 1));
          e.preventDefault();
          break;
        case "k":
        case "ArrowUp":
          setCursor((c) => Math.max(0, c - 1));
          e.preventDefault();
          break;
        case "Enter": {
          const r = rows[cursor];
          if (r) openIssue(r.id);
          e.preventDefault();
          break;
        }
        case "e": {
          const r = rows[cursor];
          if (r) {
            setInboxState.mutate({
              issueId: r.id,
              status: "done",
              snoozed_until: null,
            });
          }
          e.preventDefault();
          break;
        }
        case "h": {
          const r = rows[cursor];
          if (r) snoozeOneDay(r.id);
          e.preventDefault();
          break;
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // `rows` itself is a fresh array each render; `rows.length` +
    // `cursor` are enough to keep the handler consistent.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows.length, cursor, selectedIssueId, helpOpen]);

  return (
    <div className="flex flex-col gap-6 px-4 lg:px-6" data-testid="issues-page">
      <PageHeading
        title="Issues"
        description="Paginated issue list across every repo dev-pulse tracks. Filter by repo, state, assignee or title; click a row to edit through the §8.2 CAS path."
      />

      <Card>
        <CardContent className="flex flex-col gap-3 pt-6">
          {/* Smart-view rail (linear-projects-idea.md §3.5). The
              three buttons swap the data source / filter without
              leaving the page. Selected is signalled by `default`
              variant + `data-active="true"` for tests. */}
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant={parsed.view === "inbox" ? "default" : "outline"}
              size="sm"
              data-active={parsed.view === "inbox"}
              data-testid="issues-view-inbox"
              onClick={() => goTo({ view: "inbox" })}
            >
              <IconInbox className="mr-1 size-4" />
              My queue
            </Button>
            <Button
              variant={parsed.view === "untriaged" ? "default" : "outline"}
              size="sm"
              data-active={parsed.view === "untriaged"}
              data-testid="issues-view-untriaged"
              onClick={() => goTo({ view: "untriaged" })}
            >
              Untriaged
            </Button>
            <Button
              variant={parsed.view === "list" ? "default" : "outline"}
              size="sm"
              data-active={parsed.view === "list"}
              data-testid="issues-view-list"
              onClick={() => goTo({ view: "list" })}
            >
              All issues
            </Button>
            {/* `?` toggles the same overlay — the button is here so
                touch users can find the shortcut cheatsheet too. */}
            <Button
              variant="ghost"
              size="sm"
              className="ml-auto"
              data-testid="issues-help-trigger"
              title="Keyboard shortcuts (?)"
              onClick={() => setHelpOpen(true)}
            >
              <IconKeyboard className="mr-1 size-4" />
              Shortcuts
            </Button>
          </div>
          <div className="flex flex-wrap items-end gap-3">
            {parsed.repoId && (
              <Badge variant="secondary" className="gap-1" data-testid="issues-repo-filter">
                Repo: {repoLookup.data?.slug ?? parsed.repoId.slice(0, 8)}
                <button
                  type="button"
                  className="ml-1 rounded-sm hover:bg-muted"
                  aria-label="Clear repo filter"
                  onClick={() => goTo({ repoId: null })}
                >
                  <IconX className="size-3" />
                </button>
              </Badge>
            )}
            <div className="flex flex-col gap-1">
              <Label htmlFor="issues-state">State</Label>
              <Select
                value={parsed.state}
                onValueChange={(v) => goTo({ state: v as IssuesPageQuery["state"] })}
              >
                <SelectTrigger id="issues-state" className="w-32" data-testid="issues-state-select">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="open">Open</SelectItem>
                  <SelectItem value="closed">Closed</SelectItem>
                  <SelectItem value="all">All</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1">
              <Label htmlFor="issues-assignee">Assignee</Label>
              <Input
                id="issues-assignee"
                className="w-48"
                placeholder="login"
                value={assigneeDraft}
                onChange={(e) => setAssigneeDraft(e.target.value)}
                onBlur={() => assigneeDraft !== parsed.assignee && goTo({ assignee: assigneeDraft })}
                onKeyDown={(e) => {
                  if (e.key === "Enter") goTo({ assignee: assigneeDraft });
                }}
              />
            </div>
            <div className="flex flex-1 flex-col gap-1 min-w-64">
              <Label htmlFor="issues-q">Search title</Label>
              <Input
                id="issues-q"
                placeholder="keywords…"
                value={searchDraft}
                onChange={(e) => setSearchDraft(e.target.value)}
                onBlur={() => searchDraft !== parsed.q && goTo({ q: searchDraft })}
                onKeyDown={(e) => {
                  if (e.key === "Enter") goTo({ q: searchDraft });
                }}
                data-testid="issues-search"
              />
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardContent className="p-0">
          <Table data-testid="issues-table">
            <TableHeader className="bg-muted/50">
              <TableRow>
                <TableHead className="w-6" aria-label="Unread" />
                <TableHead className="w-16">#</TableHead>
                <TableHead>Title</TableHead>
                <TableHead className="w-24">State</TableHead>
                <TableHead className="w-48">Assignees</TableHead>
                <TableHead className="w-40">Updated</TableHead>
                <TableHead className="w-32 text-right">Inbox</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {issues.isLoading && (
                <TableRow>
                  <TableCell colSpan={7} className="text-center text-muted-foreground py-8">
                    Loading issues…
                  </TableCell>
                </TableRow>
              )}
              {issues.isError && !issues.isLoading && (
                <TableRow>
                  <TableCell colSpan={7} className="text-center text-destructive py-8">
                    Could not load issues: {issues.error instanceof Error ? issues.error.message : "unknown"}
                  </TableCell>
                </TableRow>
              )}
              {!issues.isLoading && !issues.isError && rows.length === 0 && (
                <TableRow>
                  <TableCell colSpan={7} className="text-center text-muted-foreground py-8">
                    No issues match these filters.
                  </TableCell>
                </TableRow>
              )}
              {rows.map((row, i) => (
                <TableRow
                  key={row.id}
                  className={`cursor-pointer ${i === cursor ? "bg-accent/40" : ""}`}
                  data-testid="issues-row"
                  data-cursor={i === cursor ? "true" : undefined}
                  data-unread={row.unread ? "true" : "false"}
                  onClick={() => {
                    setCursor(i);
                    openIssue(row.id);
                  }}
                >
                  <TableCell aria-label={row.unread ? "Unread" : "Read"}>
                    {/* §3.8 unread dot. Hidden when seen so the rail
                        weight matches Linear / Gmail. */}
                    {row.unread && (
                      <span
                        className="block size-2 rounded-full bg-primary"
                        data-testid="issues-row-unread-dot"
                      />
                    )}
                  </TableCell>
                  <TableCell className="font-mono text-muted-foreground">{row.number}</TableCell>
                  <TableCell>
                    <div className="flex flex-col">
                      <span className={row.unread ? "font-semibold" : "font-medium"}>
                        {row.title}
                      </span>
                      {row.repo_slug && (
                        <span className="text-xs text-muted-foreground">{row.repo_slug}</span>
                      )}
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge variant={row.state === "open" ? "default" : "secondary"}>{row.state}</Badge>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {row.assignees.length === 0 ? "—" : row.assignees.join(", ")}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {new Date(row.updated_at).toLocaleString()}
                  </TableCell>
                  <TableCell className="text-right">
                    {/* Per-row inbox actions (`linear-projects-idea.md`
                        §3.8). `e` → done; `h` → snooze 1d. `stopPropagation`
                        keeps the row-click handler from also opening the
                        peek panel. */}
                    <div className="flex justify-end gap-1">
                      <Button
                        variant="ghost"
                        size="icon"
                        title="Snooze 1 day"
                        data-testid="issues-row-snooze"
                        onClick={(e) => {
                          e.stopPropagation();
                          const wake = new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString();
                          setInboxState.mutate({
                            issueId: row.id,
                            status: "snoozed",
                            snoozed_until: wake,
                          });
                        }}
                      >
                        <IconClock className="size-4" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        title="Mark done"
                        data-testid="issues-row-done"
                        onClick={(e) => {
                          e.stopPropagation();
                          setInboxState.mutate({
                            issueId: row.id,
                            status: "done",
                            snoozed_until: null,
                          });
                        }}
                      >
                        <IconCheck className="size-4" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
        <div className="flex items-center justify-between border-t border-border px-4 py-3 text-sm text-muted-foreground">
          <span data-testid="issues-pagination-counter">
            {total === 0 ? "No issues" : `Showing ${firstShown}–${lastShown} of ${total}`}
          </span>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={parsed.offset === 0}
              onClick={() => goToOffset(Math.max(0, parsed.offset - PAGE_SIZE))}
            >
              Prev
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={parsed.offset + rows.length >= total}
              onClick={() => goToOffset(parsed.offset + PAGE_SIZE)}
            >
              Next
            </Button>
          </div>
        </div>
      </Card>

      <Sheet
        open={!!selectedIssueId}
        onOpenChange={(open) => {
          if (!open) openIssue(null);
        }}
      >
        <SheetContent className="w-full sm:max-w-2xl overflow-y-auto">
          <SheetHeader>
            <SheetTitle>Issue detail</SheetTitle>
            <SheetDescription>
              The form submits through the §8.2 optimistic-CAS path. A 409
              stale-version response reloads the row and re-prompts.
            </SheetDescription>
          </SheetHeader>
          <div className="mt-4 flex flex-col gap-3">
            {selectedIssueId && <IssueEditCard issueId={selectedIssueId} />}
            <div>
              <Button variant="ghost" size="sm" asChild>
                <a
                  href={workflowIssuesRoute({ repoId: parsed.repoId, issueId: selectedIssueId })}
                  target="_blank"
                  rel="noreferrer"
                >
                  <IconExternalLink className="mr-1 size-4" />
                  Open in new tab
                </a>
              </Button>
            </div>
          </div>
        </SheetContent>
      </Sheet>

      {/* `?` cheatsheet (linear-projects-idea.md §3.7). Plain Dialog
          so the same overlay works on touch via the toolbar button. */}
      <Dialog open={helpOpen} onOpenChange={setHelpOpen}>
        <DialogContent className="sm:max-w-md" data-testid="issues-help-dialog">
          <DialogHeader>
            <DialogTitle>Keyboard shortcuts</DialogTitle>
            <DialogDescription>
              Active when no input is focused and the peek panel is closed.
            </DialogDescription>
          </DialogHeader>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
            <dt className="font-mono text-muted-foreground">j / ↓</dt>
            <dd>Next issue</dd>
            <dt className="font-mono text-muted-foreground">k / ↑</dt>
            <dd>Previous issue</dd>
            <dt className="font-mono text-muted-foreground">Enter</dt>
            <dd>Open in peek panel</dd>
            <dt className="font-mono text-muted-foreground">Esc</dt>
            <dd>Close peek panel / help</dd>
            <dt className="font-mono text-muted-foreground">e</dt>
            <dd>Mark done</dd>
            <dt className="font-mono text-muted-foreground">h</dt>
            <dd>Snooze 1 day</dd>
            <dt className="font-mono text-muted-foreground">?</dt>
            <dd>Toggle this help</dd>
          </dl>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function IssueEditCard({ issueId }: { issueId: string }): JSX.Element {
  const issue = useIssue(issueId);
  if (issue.isLoading) {
    return <Card><CardContent>Loading issue…</CardContent></Card>;
  }
  if (issue.isError || !issue.data) {
    return (
      <Alert variant="destructive">
        <AlertTitle>Could not load issue</AlertTitle>
        <AlertDescription>
          {issue.error instanceof Error ? issue.error.message : "Unknown"}
        </AlertDescription>
      </Alert>
    );
  }
  return <IssueEditForm issue={issue.data} onReload={() => issue.refetch()} />;
}

export { IssueEditCard };

/**
 * The actual form. Holds a *form-local* copy of the editable fields
 * keyed by `formKey` — bumping `formKey` on a §8.3 reload drops the
 * controlled state and re-seeds from the latest GET, which is what
 * §8.3 wants ("ask the UI to reload and re-prompt the user").
 */
function IssueEditForm({
  issue,
  onReload,
}: {
  issue: IssueDto;
  onReload: () => void;
}): JSX.Element {
  const orgLogin = useOrgLogin(issue.org_id);
  const [formKey, setFormKey] = useState(0);
  return (
    <Card data-testid="issue-edit-card">
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>
          Issue #{issue.number} · v{issue.version}
        </CardTitle>
        <Badge variant={issue.state === "open" ? "default" : "secondary"}>
          {issue.state}
        </Badge>
      </CardHeader>
      <CardContent>
        <WritesGate orgLogin={orgLogin}>
          <IssueFormBody
            key={`${issue.id}:${issue.version}:${formKey}`}
            issue={issue}
            onStale={() => {
              setFormKey((k) => k + 1);
              onReload();
            }}
          />
        </WritesGate>
      </CardContent>
    </Card>
  );
}

function IssueFormBody({
  issue,
  onStale,
}: {
  issue: IssueDto;
  onStale: () => void;
}): JSX.Element {
  const [title, setTitle] = useState(issue.title);
  const [body, setBody] = useState(issue.body ?? "");
  const [state, setState] = useState(issue.state);
  const [comment, setComment] = useState("");
  const [staleNotice, setStaleNotice] = useState<{ currentVersion: number } | null>(null);
  const [writesNotice, setWritesNotice] = useState<string | null>(null);

  const update = useUpdateIssue(issue.id);
  const addComment = useCommentOnIssue(issue.id);

  const handleStaleVersion = (e: unknown): boolean => {
    const v = staleVersionFromError(e);
    if (v !== undefined) {
      setStaleNotice({ currentVersion: v });
      return true;
    }
    const org = writesUnavailableOrg(e);
    if (org) {
      setWritesNotice(org);
      return true;
    }
    return false;
  };

  const onSubmit = (ev: React.FormEvent): void => {
    ev.preventDefault();
    update.mutate(
      {
        expected_version: issue.version,
        title: title !== issue.title ? title : undefined,
        body: body !== (issue.body ?? "") ? body : undefined,
        state: state !== issue.state ? state : undefined,
      },
      {
        onError: handleStaleVersion,
      },
    );
  };

  const onComment = (ev: React.FormEvent): void => {
    ev.preventDefault();
    if (!comment.trim()) return;
    addComment.mutate(
      { expected_version: issue.version, body: comment },
      {
        onError: handleStaleVersion,
        onSuccess: () => setComment(""),
      },
    );
  };

  const onClose = (): void => {
    update.mutate(
      { expected_version: issue.version, state: "closed" },
      { onError: handleStaleVersion },
    );
  };
  const onReopen = (): void => {
    update.mutate(
      { expected_version: issue.version, state: "open" },
      { onError: handleStaleVersion },
    );
  };

  return (
    <div className="flex flex-col gap-4">
      {staleNotice && (
        <Alert data-testid="stale-version-notice">
          <IconAlertTriangle className="size-4" />
          <AlertTitle>This issue changed since you opened the form</AlertTitle>
          <AlertDescription className="flex flex-col gap-2">
            <span>
              The local row moved from v{issue.version} to v
              {staleNotice.currentVersion} while you were editing. Your draft
              was not applied. Reload to see the latest state, then re-apply.
            </span>
            <div>
              <Button
                size="sm"
                onClick={() => {
                  setStaleNotice(null);
                  onStale();
                }}
                data-testid="stale-version-reload"
              >
                <IconRefresh className="mr-1 size-4" />
                Reload
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      )}
      {writesNotice && (
        <Alert variant="destructive" data-testid="writes-unavailable-error">
          <AlertTitle>Writes not available for {writesNotice}</AlertTitle>
          <AlertDescription>
            The GitHub App install for this org does not have{" "}
            <code>issues: write</code>. Ask an admin to re-consent.
          </AlertDescription>
        </Alert>
      )}
      <form className="flex flex-col gap-3" onSubmit={onSubmit}>
        <input type="hidden" name="expected_version" value={issue.version} />
        <div className="flex flex-col gap-1">
          <Label>Title</Label>
          <Input value={title} onChange={(e) => setTitle(e.target.value)} />
        </div>
        <div className="flex flex-col gap-1">
          <Label>Body</Label>
          <Tabs defaultValue="preview">
            <TabsList variant="line">
              <TabsTrigger value="preview">Preview</TabsTrigger>
              <TabsTrigger value="edit">Edit</TabsTrigger>
            </TabsList>
            <TabsContent value="preview">
              {body.trim() ? (
                <div className="rounded-md border border-border bg-background px-3 py-2">
                  <Markdown>{body}</Markdown>
                </div>
              ) : (
                <p className="px-3 py-2 text-sm italic text-muted-foreground">
                  No description.
                </p>
              )}
            </TabsContent>
            <TabsContent value="edit">
              <Textarea
                value={body}
                onChange={(e) => setBody(e.target.value)}
                rows={10}
              />
            </TabsContent>
          </Tabs>
        </div>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? "Saving…" : "Save changes"}
          </Button>
          {state === "open" ? (
            <Button
              type="button"
              variant="outline"
              onClick={onClose}
              disabled={update.isPending}
            >
              Close issue
            </Button>
          ) : (
            <Button
              type="button"
              variant="outline"
              onClick={onReopen}
              disabled={update.isPending}
            >
              Reopen issue
            </Button>
          )}
          <span className="ml-auto text-xs text-muted-foreground">
            expected_version = <code>{issue.version}</code>
          </span>
        </div>
      </form>
      <IssueDatesEditor issueId={issue.id} />
      <form className="flex flex-col gap-3 border-t border-border pt-4" onSubmit={onComment}>
        <Label>Add comment</Label>
        <Textarea
          value={comment}
          onChange={(e) => setComment(e.target.value)}
          rows={3}
          placeholder="Write a comment…"
        />
        <div>
          <Button
            type="submit"
            disabled={addComment.isPending || !comment.trim()}
          >
            {addComment.isPending ? "Posting…" : "Comment"}
          </Button>
        </div>
      </form>
    </div>
  );
}

/** Resolve `org_id` → `login` from the banner data so `WritesGate`
 *  can look up the right row. Mock-aware. */
function useOrgLogin(orgId: string): string | undefined {
  return useMemo(() => {
    if (USE_MOCK) {
      return mockAppInstallBanner.orgs.find((o) => o.org_id === orgId)?.login;
    }
    return undefined;
  }, [orgId]);
}

/**
 * Start / due date pickers (§3.10). Reads the local
 * `dp_issue_dates` row via `GET /issues/{id}/dates`, writes through
 * `PATCH /issues/{id}/dates`. The PATCH is local-first — a mirror
 * failure lands on `mirror_error` and is surfaced as a non-blocking
 * footnote so the user knows the local save committed.
 *
 * The two inputs use `type="date"` so the picker stays
 * accessible / keyboard-friendly without pulling a calendar
 * dependency. Wire side carries RFC3339 instants; we serialise the
 * local `YYYY-MM-DD` as `T00:00:00Z` (start) / `T23:59:59Z` (due)
 * so a single-day deadline sorts correctly against `now()` in the
 * `Due this week` / `Overdue` smart-view filters.
 */
function IssueDatesEditor({ issueId }: { issueId: string }): JSX.Element {
  const dates = useIssueDates(issueId);
  const update = useUpdateIssueDates(issueId);
  const [start, setStart] = useState<string>("");
  const [due, setDue] = useState<string>("");
  const [error, setError] = useState<string | null>(null);

  // Seed local inputs from the server row whenever it lands. We do
  // not re-seed on every render so the user can edit freely without
  // the input snapping back to the server value mid-keystroke.
  const loaded = dates.data?.updated_at;
  useEffect(() => {
    if (!dates.data) return;
    setStart(dates.data.start_at ? dates.data.start_at.slice(0, 10) : "");
    setDue(dates.data.due_at ? dates.data.due_at.slice(0, 10) : "");
    setError(null);
  }, [loaded, dates.data]);

  const submit = (ev: React.FormEvent): void => {
    ev.preventDefault();
    setError(null);
    const startAt = start ? `${start}T00:00:00Z` : null;
    const dueAt = due ? `${due}T23:59:59Z` : null;
    if (startAt && dueAt && startAt > dueAt) {
      setError("Start date must be on or before the due date.");
      return;
    }
    update.mutate(
      { start_at: startAt, due_at: dueAt },
      {
        onError: (e) => {
          setError(e instanceof Error ? e.message : "Could not save dates.");
        },
      },
    );
  };

  const clearAll = (): void => {
    setStart("");
    setDue("");
    setError(null);
    update.mutate({ start_at: null, due_at: null });
  };

  return (
    <form
      className="flex flex-col gap-2 border-t border-border pt-4"
      onSubmit={submit}
      data-testid="issue-dates-editor"
    >
      <Label className="text-sm">Dates</Label>
      <div className="flex flex-wrap items-end gap-3">
        <div className="flex flex-col gap-1">
          <span className="text-xs text-muted-foreground">Start</span>
          <Input
            type="date"
            value={start}
            onChange={(e) => setStart(e.target.value)}
            data-testid="issue-dates-start"
            className="w-40"
          />
        </div>
        <div className="flex flex-col gap-1">
          <span className="text-xs text-muted-foreground">Due</span>
          <Input
            type="date"
            value={due}
            onChange={(e) => setDue(e.target.value)}
            data-testid="issue-dates-due"
            className="w-40"
          />
        </div>
        <Button
          type="submit"
          size="sm"
          disabled={update.isPending}
          data-testid="issue-dates-save"
        >
          {update.isPending ? "Saving…" : "Save dates"}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          onClick={clearAll}
          disabled={update.isPending || (!start && !due)}
        >
          Clear
        </Button>
      </div>
      {error && (
        <p
          className="text-xs text-destructive"
          data-testid="issue-dates-error"
          role="alert"
        >
          {error}
        </p>
      )}
      {dates.data?.mirror_error && !error && (
        <p
          className="text-xs text-muted-foreground"
          data-testid="issue-dates-mirror-error"
        >
          Saved locally. Projects v2 mirror failed: {dates.data.mirror_error}
        </p>
      )}
    </form>
  );
}

