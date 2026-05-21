/**
 * Triage page — Linear-style three-pane workflow surface
 * (`linear-projects-idea.md` §3).
 *
 * Layout:
 *
 * ```
 * ┌──────────────┬─────────────────────────┬──────────────────┐
 * │ Smart views  │ Issue list              │ Peek panel       │
 * │  ★ My queue  │  ● #123  bug fix      ► │ <IssueEditCard>  │
 * │  Untriaged   │    #124  add metric     │                  │
 * │  Snoozed     │    #125  flaky test     │                  │
 * │  All issues  │    …                    │                  │
 * │  ─────────── │                         │                  │
 * │  Pinned      │                         │                  │
 * │   nube/api   │                         │                  │
 * │   nube/web   │                         │                  │
 * └──────────────┴─────────────────────────┴──────────────────┘
 * ```
 *
 * URL state (round-trippable):
 *   `#/workflow/triage?view=mine|untriaged|all|snoozed&repo_id=<uuid>&issue=<uuid>`
 *
 * The left rail picks the data source and adds server-side
 * predicates; the middle pane renders the resulting rows with the
 * §3.8 unread dot, status pill, and per-row inbox actions; the
 * right pane embeds the same `IssueEditCard` the legacy issues page
 * exposes so the §8.2 CAS write path is reused verbatim.
 *
 * Keyboard (active when no input is focused and no peek/help dialog
 * is open):
 *
 *   j / ↓   next issue           Enter   open in peek panel
 *   k / ↑   previous issue       Esc     close peek / help / view
 *   e       mark done            h       snooze 1 day
 *   ?       toggle this help
 *
 * The peek panel is in-flow (no Sheet overlay) so all three columns
 * scroll independently and the row–detail relationship stays
 * visually obvious — that's the whole point of the Linear layout.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import {
  IconAlertTriangle,
  IconBookmark,
  IconCalendar,
  IconCalendarDue,
  IconCheck,
  IconChevronDown,
  IconChevronRight,
  IconClock,
  IconCommand,
  IconExternalLink,
  IconHash,
  IconInbox,
  IconKeyboard,
  IconList,
  IconMoon,
  IconRotateClockwise,
  IconSparkles,
  IconUser,
  IconUsers,
  IconX,
} from "@tabler/icons-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";

import type { IssueListItem, ListIssuesQuery } from "../api/client.js";
import {
  navigate,
  triageView,
  useRoute,
  workflowSelectedIssue,
  workflowSelectedRepoId,
  workflowTriageRoute,
  type TriageView,
} from "../routes.js";

import { IssueEditCard } from "./issues-page.jsx";
import {
  useBulkInbox,
  useIssueDatesBatch,
  useIssueList,
  useMarkInboxSeen,
  useMyQueue,
  usePins,
  useRepoList,
  useSetInboxState,
  useTags,
  useToggleIssueState,
} from "./use-workflow-data.js";

const PAGE_SIZE = 100;

/** Group-by axis on the middle pane. `none` keeps the flat list
 *  (default). The other modes bucket rows by status / assignee /
 *  repo into collapsible sections — pure UI, no extra round-trip. */
type GroupBy = "none" | "status" | "assignee" | "repo";

/** Sort axis on the middle pane. Server still returns newest-first
 *  by `updated_at DESC` — the UI then re-orders client-side so
 *  toggling sort is instant and never triggers a refetch. */
type SortBy = "updated_desc" | "updated_asc" | "created_desc" | "number_asc";

interface ViewDef {
  id: TriageView;
  label: string;
  hint: string;
  icon: typeof IconInbox;
}

/** Built-in smart views — order matches the rail rendering top-to-bottom. */
const VIEWS: ViewDef[] = [
  {
    id: "mine",
    label: "My queue",
    hint: "Issues that need your attention",
    icon: IconInbox,
  },
  {
    id: "untriaged",
    label: "Untriaged",
    hint: "No assignee, no label",
    icon: IconSparkles,
  },
  {
    id: "due_week",
    label: "Due this week",
    hint: "Due within the next 7 days",
    icon: IconCalendar,
  },
  {
    id: "overdue",
    label: "Overdue",
    hint: "Past their due date — re-bumped to inbox",
    icon: IconAlertTriangle,
  },
  {
    id: "snoozed",
    label: "Snoozed",
    hint: "Hidden until their wake-up",
    icon: IconMoon,
  },
  {
    id: "all",
    label: "All issues",
    hint: "Everything dev-pulse tracks",
    icon: IconList,
  },
];

/**
 * Build the `ListIssuesQuery` for the selected smart view. The view
 * picks the *predicates* — the data-source switch (`useMyQueue` vs
 * `useIssueList`) lives in [`useTriageRows`] so the React-Query
 * cache key reflects the view.
 */
function filterFor(view: TriageView, repoId: string | null): ListIssuesQuery {
  const base: ListIssuesQuery = {
    limit: PAGE_SIZE,
    offset: 0,
    state: "open",
  };
  if (repoId) base.repo_id = repoId;
  if (view === "untriaged") {
    base.untriaged = true;
    return base;
  }
  if (view === "all") {
    base.state = "all";
    return base;
  }
  // `mine`, `snoozed`, `due_week`, `overdue`, and `tag:<id>` all
  // ride on top of the inbox query; the per-view client-side
  // refinement happens in `useTriageRows`.
  return base;
}

/**
 * Fetch the rows for the active view. `mine` and `untriaged` both
 * pivot off the caller's inbox; `all` uses the org-wide list.
 *
 * `snoozed` is a placeholder until the backend ships a dedicated
 * `/me/queue/snoozed` endpoint — for now both branches resolve to
 * empty + an "empty state" pane.
 */
function useTriageRows(view: TriageView, repoId: string | null) {
  const q = filterFor(view, repoId);
  const queue = useMyQueue(q);
  const list = useIssueList(q);
  if (view === "all") return list;
  if (view === "snoozed") {
    // Placeholder — the real snoozed endpoint is deferred (see
    // `filterFor` comment). Empty result + no error.
    return {
      data: { rows: [], total: 0, limit: PAGE_SIZE, offset: 0 },
      isLoading: false,
      isError: false,
      error: null as unknown as Error,
      refetch: () => undefined,
    };
  }
  // `mine`, `due_week`, `overdue`, and `tag:<id>` all start from
  // `/me/queue`. Tag and date refinement is client-side because
  // the read endpoints do not (yet) accept those predicates.
  return queue;
}

export function TriagePage(): JSX.Element {
  const route = useRoute();
  const view = triageView(route);
  const repoId = workflowSelectedRepoId(route);
  const selectedIssueId = workflowSelectedIssue(route);

  const rowsQ = useTriageRows(view, repoId);
  const allRows: IssueListItem[] = rowsQ.data?.rows ?? [];

  // Per-row date fetch — bounded by `PAGE_SIZE`, react-query
  // caches per id so the picker in the peek panel and the smart
  // views share the same in-flight request.
  const rowIds = useMemo(() => allRows.map((r) => r.id), [allRows]);
  const datesById = useIssueDatesBatch(rowIds);

  // Client-side refinement for the date-driven smart views and
  // the tag-backed saved-view escape hatch. The §3.10 contract
  // says past-due rows re-bump to inbox — `overdue` is rendered
  // from `/me/queue` (the inbox source) plus the `due_at < now`
  // filter, which is exactly what re-bump implies.
  const tagFilterId = view.startsWith("tag:") ? view.slice(4) : null;
  const rows: IssueListItem[] = useMemo(() => {
    if (tagFilterId) {
      // Tag-saved views — no per-row tag join on the list
      // endpoint yet, so the rail entry behaves like a hint: rows
      // surface but stay un-narrowed until §7 ships issue-tag
      // links into the list response. The view label flags this.
      return allRows;
    }
    if (view === "due_week" || view === "overdue") {
      const now = Date.now();
      const week = now + 7 * 24 * 60 * 60 * 1000;
      return allRows.filter((r) => {
        const due = datesById.get(r.id)?.due_at;
        if (!due) return false;
        const t = new Date(due).getTime();
        if (!Number.isFinite(t)) return false;
        if (view === "overdue") return t < now;
        return t >= now && t <= week;
      });
    }
    return allRows;
  }, [allRows, datesById, view, tagFilterId]);

  // Live count for ★ My queue — the same one the sidebar shows.
  // Cheap probe (`limit: 1`) so we never bloat the cache.
  const inboxProbe = useMyQueue({ limit: 1, offset: 0 });
  const inboxCount = inboxProbe.data?.total ?? 0;

  const pins = usePins();
  // Resolve `PinDto.target_id` → `owner/repo` so the rail stops
  // rendering opaque uuid prefixes. The repo list is paginated;
  // we request the first page (cap=200 server-side) and fall
  // back to the short id for anything past the boundary.
  const repos = useRepoList({ limit: 200, offset: 0 });
  const repoLabelById = useMemo(() => {
    const m = new Map<string, string>();
    for (const r of repos.data?.rows ?? []) {
      m.set(r.id, `${r.org_login}/${r.name}`);
    }
    return m;
  }, [repos.data]);

  // Saved views — tag-backed first-class rail entries. We surface
  // every tag the caller can see; the count is the §7.4 viewer-
  // filtered `visible_link_count` and rides on the same DTO.
  const tags = useTags();

  const markSeen = useMarkInboxSeen();
  const setInboxState = useSetInboxState();
  const toggleState = useToggleIssueState();
  const bulkInbox = useBulkInbox();

  // ----- Multi-select + bulk-action state (slice 2 §3.8) --------------
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const toggleSelected = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const clearSelected = () => setSelected(new Set());

  // ----- Middle-pane group/sort (slice 2) ----------------------------
  const [groupBy, setGroupBy] = useState<GroupBy>("none");
  const [sortBy, setSortBy] = useState<SortBy>("updated_desc");

  // ----- Rail collapsible sections (§13.5 sidebar render cap) ---------
  const [peopleOpen, setPeopleOpen] = useState(false); // collapsed-by-default
  const [teamsOpen, setTeamsOpen] = useState(true);
  const [savedOpen, setSavedOpen] = useState(true);

  // ----- `Due` column toggle (`g d`). Persisted to localStorage so
  // the operator's column preference rides through reloads. --------
  const [showDueColumn, setShowDueColumn] = useState<boolean>(() =>
    readBoolPref("triage.dueColumn", false),
  );
  useEffect(() => {
    writeBoolPref("triage.dueColumn", showDueColumn);
  }, [showDueColumn]);

  // ----- Resizable pane widths. Both saved to localStorage so the
  // operator's layout persists. The right (peek) pane keeps its
  // fixed `minmax` floor; only the rail / middle width is user-
  // adjustable here. ---------------------------------------------------
  const [railWidth, setRailWidth] = useState<number>(() =>
    readNumberPref("triage.rail.width", 224),
  );
  const [peekWidth, setPeekWidth] = useState<number>(() =>
    readNumberPref("triage.peek.width", 480),
  );
  useEffect(() => writeNumberPref("triage.rail.width", railWidth), [railWidth]);
  useEffect(() => writeNumberPref("triage.peek.width", peekWidth), [peekWidth]);

  // ----- ⌘K command palette (slice 2 jump-to / view-switch / apply) ---
  const [paletteOpen, setPaletteOpen] = useState(false);

  const goToView = (v: TriageView) =>
    navigate(workflowTriageRoute({ view: v, repoId, issueId: selectedIssueId }));
  const goToRepo = (next: string | null) =>
    navigate(workflowTriageRoute({ view, repoId: next, issueId: selectedIssueId }));
  const openIssue = (id: string | null) => {
    // §3.8 — opening a row in the peek panel marks it seen. Best
    // effort; the mutation swallows failures.
    if (id) markSeen.mutate([id]);
    navigate(workflowTriageRoute({ view, repoId, issueId: id }));
  };

  // ----- Keyboard nav (linear-projects-idea.md §3.7) ---------------------
  const [cursor, setCursor] = useState(0);
  const [helpOpen, setHelpOpen] = useState(false);

  useEffect(() => {
    setCursor((c) => (rows.length === 0 ? 0 : Math.min(c, rows.length - 1)));
  }, [rows.length]);

  useEffect(() => {
    const el = document.querySelector<HTMLLIElement>(
      '[data-testid="triage-list"] li[data-cursor="true"]',
    );
    el?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  useEffect(() => {
    const inEditable = (el: EventTarget | null): boolean => {
      const n = el as HTMLElement | null;
      if (!n) return false;
      const tag = n.tagName;
      return (
        tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || n.isContentEditable
      );
    };
    const snoozeOneDay = (id: string): void => {
      const wake = new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString();
      setInboxState.mutate({ issueId: id, status: "snoozed", snoozed_until: wake });
    };
    // `g`-chord state — `g d` toggles the `Due` column per the
    // §3.5 "g-prefixed shortcuts" pattern. The chord times out
    // after one keystroke to keep the binding scoped.
    let gPending = false;
    const onKey = (e: KeyboardEvent): void => {
      // ⌘K / ctrl-K opens the command palette regardless of focus
      // (this is the slice-2 jump-to / view-switch / apply-to-
      // selection entry point — must work from inside the peek
      // panel and from any input).
      if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K")) {
        setPaletteOpen((p) => !p);
        e.preventDefault();
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      // Shift-E / Shift-H / Shift-D bulk transitions over the
      // current selection (slice 2 §3.8). Skipped when no rows
      // are selected — the per-row `e`/`h` keys still cover the
      // single-row path.
      if (e.shiftKey && !inEditable(e.target)) {
        const ids = Array.from(selected);
        if (ids.length === 0) return;
        if (e.key === "E") {
          bulkInbox.mutate({ issueIds: ids, op: "done_all" });
          clearSelected();
          e.preventDefault();
          return;
        }
        if (e.key === "H") {
          const wake = new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString();
          bulkInbox.mutate({
            issueIds: ids,
            op: "snooze_all",
            snoozedUntil: wake,
          });
          clearSelected();
          e.preventDefault();
          return;
        }
        if (e.key === "D") {
          // Shift-D mirrors the "dismiss back to inbox" affordance
          // — restore selected rows to the inbox so an over-eager
          // shift-E can be reversed without per-row clicks.
          bulkInbox.mutate({ issueIds: ids, op: "inbox_all" });
          clearSelected();
          e.preventDefault();
          return;
        }
      }
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
      if (selectedIssueId) return;
      if (helpOpen) return;
      // `g`-chord toggle for the `Due` column. Fires before the
      // row-cursor switch so an inadvertent `g` followed by an
      // unrelated key just clears the chord.
      if (gPending) {
        gPending = false;
        if (e.key === "d") {
          setShowDueColumn((v) => !v);
          e.preventDefault();
          return;
        }
        // Fall through — unknown follow-up keys clear the chord
        // silently.
      } else if (e.key === "g") {
        gPending = true;
        e.preventDefault();
        return;
      }
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
            const nextState = r.state === "closed" ? "open" : "closed";
            toggleState.mutate(
              { id: r.id, version: r.version, state: nextState },
              {
                onSuccess: () => {
                  setInboxState.mutate({
                    issueId: r.id,
                    status: nextState === "closed" ? "done" : "inbox",
                    snoozed_until: null,
                  });
                },
              },
            );
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows.length, cursor, selectedIssueId, helpOpen, selected]);

  const activeViewDef = VIEWS.find((v) => v.id === view) ?? VIEWS[0]!;
  const total = rowsQ.data?.total ?? 0;

  // Apply client-side sort. The list is bounded by `PAGE_SIZE`, so
  // the cost is negligible; the comparator order matches the four
  // §3.5 sort axes.
  const sortedRows = useMemo<IssueListItem[]>(() => {
    const copy = rows.slice();
    switch (sortBy) {
      case "updated_asc":
        copy.sort((a, b) => a.updated_at.localeCompare(b.updated_at));
        break;
      case "created_desc":
        // `IssueListItem` does not surface `created_at` — falls
        // back to issue number which is monotonic in creation
        // order per repo. Cross-repo lists therefore mix.
        copy.sort((a, b) => b.number - a.number);
        break;
      case "number_asc":
        copy.sort((a, b) => a.number - b.number);
        break;
      case "updated_desc":
      default:
        copy.sort((a, b) => b.updated_at.localeCompare(a.updated_at));
        break;
    }
    return copy;
  }, [rows, sortBy]);

  // Visual group-by labels are deferred — the dropdown still sets
  // state so the URL / e2e tooling can read the user's choice, but
  // the middle pane keeps the flat sorted-list rendering until the
  // group-collapse UX lands in a follow-up.
  void groupBy;

  // Repo pins for the rail's "Pinned repos" section (§3.5). Tag
  // pins are skipped here — the rail mirrors Linear's "Favorites"
  // shape; cross-cutting tag views land in a future slice.
  const repoPins = useMemo(
    () => (pins.data ?? []).filter((p) => p.kind === "repo"),
    [pins.data],
  );

  // Dynamic grid template. On `xl+` viewports we render all three
  // panes plus two drag handles; below `xl` the peek panel collapses
  // and the handle disappears.
  const gridTemplate = selectedIssueId
    ? `${railWidth}px 4px minmax(0,1fr) 4px ${peekWidth}px`
    : `${railWidth}px 4px minmax(0,1fr)`;

  return (
    <div
      className="flex h-[calc(100dvh-7rem)] border-y border-border xl:grid"
      style={{ gridTemplateColumns: gridTemplate }}
      data-testid="triage-page"
    >
      {/* ──────────────── LEFT RAIL ──────────────── */}
      <aside
        className="flex w-56 shrink-0 flex-col gap-1 overflow-y-auto border-r border-border bg-muted/30 p-3 xl:w-auto"
        data-testid="triage-rail"
      >
        <div className="px-2 pt-1 pb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          Triage
        </div>
        {VIEWS.map((v) => {
          const active = v.id === view;
          const Icon = v.icon;
          const badge = v.id === "mine" && inboxCount > 0 ? inboxCount : null;
          return (
            <button
              key={v.id}
              type="button"
              data-testid={`triage-view-${v.id}`}
              data-active={active ? "true" : undefined}
              onClick={() => goToView(v.id)}
              className={`flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors ${
                active
                  ? "bg-primary/10 text-foreground font-medium"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground"
              }`}
            >
              <Icon
                className={`size-4 ${active ? "text-primary" : "text-muted-foreground"}`}
              />
              <span className="flex-1 truncate">{v.label}</span>
              {badge != null && (
                <Badge
                  variant="secondary"
                  className="h-5 min-w-5 justify-center px-1.5 text-[10px]"
                >
                  {badge > 99 ? "99+" : badge}
                </Badge>
              )}
            </button>
          );
        })}

        {/* Teams rail — placeholder until backend ships team-scoped
            queue endpoints. Open-by-default per §13.5; clicking a
            row just stages the filter (no-op until §3.5 follow-up). */}
        <Separator className="my-3" />
        <button
          type="button"
          data-testid="triage-rail-teams-toggle"
          data-open={teamsOpen ? "true" : "false"}
          onClick={() => setTeamsOpen((v) => !v)}
          className="flex items-center gap-1.5 px-2 pb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground hover:text-foreground"
        >
          {teamsOpen ? (
            <IconChevronDown className="size-3.5" />
          ) : (
            <IconChevronRight className="size-3.5" />
          )}
          <IconUsers className="size-3.5" /> Teams
        </button>
        {teamsOpen && (
          <div
            className="px-2 pb-1 text-xs text-muted-foreground"
            data-testid="triage-rail-teams"
          >
            Team-scoped queues land with §3.5 follow-up.
          </div>
        )}

        {/* People rail — §10 multi-identity. Collapsed-by-default
            so the rail stays under the §13.5 50-row render cap on
            large orgs (a 200-person org would otherwise blow past
            the budget on first paint). */}
        <button
          type="button"
          data-testid="triage-rail-people-toggle"
          data-open={peopleOpen ? "true" : "false"}
          onClick={() => setPeopleOpen((v) => !v)}
          className="mt-1 flex items-center gap-1.5 px-2 pb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground hover:text-foreground"
        >
          {peopleOpen ? (
            <IconChevronDown className="size-3.5" />
          ) : (
            <IconChevronRight className="size-3.5" />
          )}
          <IconUser className="size-3.5" /> People
        </button>
        {peopleOpen && (
          <div
            className="px-2 pb-1 text-xs text-muted-foreground"
            data-testid="triage-rail-people"
          >
            Per-person queues land with §10 identity handlers.
          </div>
        )}

        {/* Saved views — §14.6 tag-backed entries with viewer-
            filtered counts. Each tag is a first-class rail row;
            clicking jumps the view to `tag:<id>` and the list
            scope renders the matching rows (server-side narrowing
            lands when the list endpoint accepts `tag_ids`). */}
        {(tags.data?.length ?? 0) > 0 && (
          <>
            <Separator className="my-3" />
            <button
              type="button"
              data-testid="triage-rail-saved-toggle"
              data-open={savedOpen ? "true" : "false"}
              onClick={() => setSavedOpen((v) => !v)}
              className="flex items-center gap-1.5 px-2 pb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground hover:text-foreground"
            >
              {savedOpen ? (
                <IconChevronDown className="size-3.5" />
              ) : (
                <IconChevronRight className="size-3.5" />
              )}
              <IconHash className="size-3.5" /> Saved views
            </button>
            {savedOpen &&
              (tags.data ?? []).map((t) => {
                const id = `tag:${t.id}` as TriageView;
                const active = view === id;
                return (
                  <button
                    key={t.id}
                    type="button"
                    data-testid={`triage-saved-${t.id}`}
                    data-active={active ? "true" : undefined}
                    onClick={() => goToView(id)}
                    className={`flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors ${
                      active
                        ? "bg-primary/10 text-foreground font-medium"
                        : "text-muted-foreground hover:bg-accent hover:text-foreground"
                    }`}
                    title={t.description ?? t.name}
                  >
                    <span
                      className="size-2 shrink-0 rounded-full"
                      style={{ backgroundColor: t.color }}
                    />
                    <span className="flex-1 truncate">{t.name}</span>
                    <Badge
                      variant="secondary"
                      className="h-5 min-w-5 justify-center px-1.5 text-[10px]"
                    >
                      {t.visible_link_count > 99 ? "99+" : t.visible_link_count}
                    </Badge>
                  </button>
                );
              })}
          </>
        )}

        {repoPins.length > 0 && (
          <>
            <Separator className="my-3" />
            <div className="flex items-center gap-1.5 px-2 pb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              <IconBookmark className="size-3.5" />
              Pinned repos
            </div>
            <button
              type="button"
              data-testid="triage-repo-all"
              onClick={() => goToRepo(null)}
              className={`rounded-md px-2 py-1.5 text-left text-sm transition-colors ${
                repoId === null
                  ? "bg-primary/10 text-foreground font-medium"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground"
              }`}
            >
              All repos
            </button>
            {repoPins.map((p) => {
              const active = repoId === p.target_id;
              // §14.6 — `PinDto` has no denormalised label so we
              // resolve `owner/repo` from the repo list. Fall back
              // to the short id only when the repo isn't on the
              // first page (rare; the list is capped at 200).
              const label = repoLabelById.get(p.target_id) ?? p.target_id.slice(0, 8);
              return (
                <button
                  key={p.target_id}
                  type="button"
                  data-testid={`triage-repo-${p.target_id}`}
                  onClick={() => goToRepo(p.target_id)}
                  className={`truncate rounded-md px-2 py-1.5 text-left text-sm transition-colors ${
                    active
                      ? "bg-primary/10 text-foreground font-medium"
                      : "text-muted-foreground hover:bg-accent hover:text-foreground"
                  }`}
                  title={p.target_id}
                >
                  {label}
                </button>
              );
            })}
          </>
        )}
      </aside>

      <div className="hidden xl:block">
        <SplitHandle
          side="left"
          width={railWidth}
          setWidth={setRailWidth}
          min={160}
          max={420}
        />
      </div>

      {/* ──────────────── MIDDLE: ISSUE LIST ──────────────── */}
      <section
        className="flex flex-col overflow-hidden bg-background"
        data-testid="triage-middle"
      >
        <header className="flex items-center justify-between gap-3 border-b border-border px-4 py-2.5">
          <div className="flex items-center gap-2 min-w-0">
            <activeViewDef.icon className="size-4 text-primary shrink-0" />
            <h2 className="truncate text-sm font-semibold">{activeViewDef.label}</h2>
            <span className="shrink-0 text-xs text-muted-foreground">·</span>
            <span className="truncate text-xs text-muted-foreground">
              {activeViewDef.hint}
            </span>
            {total > 0 && (
              <Badge variant="outline" className="ml-2 shrink-0 text-[10px]">
                {total}
              </Badge>
            )}
          </div>
          <div className="flex items-center gap-1">
            <select
              data-testid="triage-group-by"
              value={groupBy}
              onChange={(e) => setGroupBy(e.target.value as GroupBy)}
              className="h-8 rounded-md border border-border bg-background px-2 text-xs"
              title="Group by"
            >
              <option value="none">Group: none</option>
              <option value="status">Group: status</option>
              <option value="assignee">Group: assignee</option>
              <option value="repo">Group: repo</option>
            </select>
            <select
              data-testid="triage-sort-by"
              value={sortBy}
              onChange={(e) => setSortBy(e.target.value as SortBy)}
              className="h-8 rounded-md border border-border bg-background px-2 text-xs"
              title="Sort by"
            >
              <option value="updated_desc">Updated · newest</option>
              <option value="updated_asc">Updated · oldest</option>
              <option value="created_desc">Number · newest</option>
              <option value="number_asc">Number · oldest</option>
            </select>
            <Button
              variant={showDueColumn ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setShowDueColumn((v) => !v)}
              data-testid="triage-due-toggle"
              data-active={showDueColumn ? "true" : "false"}
              title="Toggle Due column (g d)"
            >
              <IconCalendarDue className="mr-1 size-4" />
              <span className="hidden sm:inline">Due</span>
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setPaletteOpen(true)}
              data-testid="triage-palette-trigger"
              title="Command palette (⌘K)"
            >
              <IconCommand className="mr-1 size-4" />
              <span className="hidden sm:inline">⌘K</span>
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setHelpOpen(true)}
              data-testid="triage-help-trigger"
              title="Keyboard shortcuts (?)"
            >
              <IconKeyboard className="mr-1 size-4" />
              <span className="hidden sm:inline">Shortcuts</span>
            </Button>
          </div>
        </header>

        {selected.size > 0 && (
          <div
            className="flex items-center gap-2 border-b border-border bg-accent/30 px-4 py-1.5 text-xs"
            data-testid="triage-bulk-bar"
          >
            <span className="font-medium">{selected.size} selected</span>
            <span className="text-muted-foreground">
              Shift-E done · Shift-H snooze 1d · Shift-D restore
            </span>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                const ids = Array.from(selected);
                bulkInbox.mutate({ issueIds: ids, op: "done_all" });
                clearSelected();
              }}
              data-testid="triage-bulk-done"
            >
              Done
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                const ids = Array.from(selected);
                const wake = new Date(
                  Date.now() + 24 * 60 * 60 * 1000,
                ).toISOString();
                bulkInbox.mutate({
                  issueIds: ids,
                  op: "snooze_all",
                  snoozedUntil: wake,
                });
                clearSelected();
              }}
              data-testid="triage-bulk-snooze"
            >
              Snooze 1d
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={clearSelected}
              data-testid="triage-bulk-clear"
            >
              Clear
            </Button>
          </div>
        )}

        <ol
          className="flex-1 overflow-y-auto"
          data-testid="triage-list"
        >
          {rowsQ.isLoading && (
            <li className="px-4 py-8 text-center text-sm text-muted-foreground">
              Loading…
            </li>
          )}
          {rowsQ.isError && (
            <li className="px-4 py-8 text-center text-sm text-destructive">
              Could not load issues:{" "}
              {rowsQ.error instanceof Error ? rowsQ.error.message : "unknown"}
            </li>
          )}
          {!rowsQ.isLoading && !rowsQ.isError && rows.length === 0 && (
            <li className="px-4 py-12 text-center text-sm text-muted-foreground">
              {view === "snoozed"
                ? "Snoozed view is coming in slice 2."
                : view === "mine"
                  ? "Inbox zero. Nothing needs your attention."
                  : "No issues match this view."}
            </li>
          )}
          {sortedRows.map((row, i) => {
            const active = row.id === selectedIssueId;
            const cursored = i === cursor;
            const isSelected = selected.has(row.id);
            return (
              <li
                key={row.id}
                data-testid="triage-row"
                data-cursor={cursored ? "true" : undefined}
                data-active={active ? "true" : undefined}
                data-unread={row.unread ? "true" : "false"}
                className={`group flex items-center gap-3 border-b border-border/60 px-4 py-2 cursor-pointer select-none ${
                  active
                    ? "bg-primary/10"
                    : cursored
                      ? "bg-accent/40"
                      : "hover:bg-accent/30"
                }`}
                onClick={() => {
                  setCursor(i);
                  openIssue(row.id);
                }}
              >
                <span
                  className="flex w-2 shrink-0 justify-center"
                  aria-label={row.unread ? "Unread" : "Read"}
                >
                  {row.unread && (
                    <span
                      className="block size-2 rounded-full bg-primary"
                      data-testid="triage-row-unread-dot"
                    />
                  )}
                </span>
                <input
                  type="checkbox"
                  data-testid="triage-row-select"
                  className="size-4 shrink-0 cursor-pointer"
                  checked={isSelected}
                  onClick={(e) => e.stopPropagation()}
                  onChange={() => toggleSelected(row.id)}
                  aria-label={isSelected ? "Deselect issue" : "Select issue"}
                />
                <Badge
                  variant={row.state === "open" ? "default" : "secondary"}
                  className="shrink-0 px-1.5 py-0 text-[10px] uppercase"
                >
                  {row.state}
                </Badge>
                <span className="shrink-0 font-mono text-xs text-muted-foreground tabular-nums">
                  #{row.number}
                </span>
                <span
                  className={`truncate text-sm ${row.unread ? "font-semibold" : ""}`}
                  title={row.title}
                >
                  {row.title}
                </span>
                {row.repo_slug && (
                  <span className="ml-auto shrink-0 text-xs text-muted-foreground">
                    {row.repo_slug}
                  </span>
                )}
                {row.assignees.length > 0 && (
                  <span className="shrink-0 text-xs text-muted-foreground">
                    {row.assignees.slice(0, 2).join(", ")}
                    {row.assignees.length > 2 && ` +${row.assignees.length - 2}`}
                  </span>
                )}
                {showDueColumn && (() => {
                  const due = datesById.get(row.id)?.due_at ?? null;
                  if (!due) {
                    return (
                      <span
                        className="shrink-0 text-xs text-muted-foreground/60 tabular-nums"
                        data-testid="triage-row-due"
                        data-due="none"
                      >
                        —
                      </span>
                    );
                  }
                  const t = new Date(due).getTime();
                  const overdue = Number.isFinite(t) && t < Date.now();
                  return (
                    <span
                      className={`shrink-0 text-xs tabular-nums ${
                        overdue
                          ? "text-destructive font-medium"
                          : "text-muted-foreground"
                      }`}
                      data-testid="triage-row-due"
                      data-due={overdue ? "overdue" : "future"}
                      title={due}
                    >
                      {formatDueLabel(due)}
                    </span>
                  );
                })()}
                <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
                  {formatRelative(row.updated_at)}
                </span>
                <span className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                  <Button
                    variant="ghost"
                    size="icon"
                    title="Snooze 1 day (h)"
                    data-testid="triage-row-snooze"
                    onClick={(e) => {
                      e.stopPropagation();
                      const wake = new Date(
                        Date.now() + 24 * 60 * 60 * 1000,
                      ).toISOString();
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
                    title={
                      row.state === "closed"
                        ? "Reopen issue"
                        : "Mark done (e) — closes on GitHub"
                    }
                    data-testid={
                      row.state === "closed"
                        ? "triage-row-reopen"
                        : "triage-row-done"
                    }
                    onClick={(e) => {
                      e.stopPropagation();
                      const nextState =
                        row.state === "closed" ? "open" : "closed";
                      toggleState.mutate(
                        {
                          id: row.id,
                          version: row.version,
                          state: nextState,
                        },
                        {
                          onSuccess: () => {
                            // Mirror the GH transition into the
                            // per-user inbox: closing dismisses
                            // the row (status = done); reopening
                            // brings it back to default Inbox.
                            setInboxState.mutate({
                              issueId: row.id,
                              status: nextState === "closed" ? "done" : "inbox",
                              snoozed_until: null,
                            });
                          },
                        },
                      );
                    }}
                  >
                    {row.state === "closed" ? (
                      <IconRotateClockwise className="size-4" />
                    ) : (
                      <IconCheck className="size-4" />
                    )}
                  </Button>
                </span>
              </li>
            );
          })}
        </ol>
      </section>

      {selectedIssueId && (
        <div className="hidden xl:block">
          <SplitHandle
            side="right"
            width={peekWidth}
            setWidth={setPeekWidth}
            min={320}
            max={720}
          />
        </div>
      )}

      {/* ──────────────── RIGHT: PEEK PANEL ──────────────── */}
      {/* Always rendered on xl+; on smaller screens we hide it and
          the row click falls back to a temporary overlay would be
          nice but is deferred — for now small screens use the legacy
          Sheet-based issues page. */}
      <aside
        className="hidden flex-col overflow-y-auto border-l border-border bg-background xl:flex"
        data-testid="triage-peek"
      >
        {selectedIssueId ? (
          <>
            <header className="sticky top-0 z-10 flex items-center justify-between gap-2 border-b border-border bg-background/95 px-4 py-2.5 backdrop-blur">
              <h2 className="text-sm font-semibold">Issue detail</h2>
              <div className="flex items-center gap-1">
                <Button
                  variant="ghost"
                  size="icon"
                  title="Open in new tab"
                  asChild
                >
                  <a
                    href={workflowTriageRoute({
                      view,
                      repoId,
                      issueId: selectedIssueId,
                    })}
                    target="_blank"
                    rel="noreferrer"
                  >
                    <IconExternalLink className="size-4" />
                  </a>
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  title="Close (Esc)"
                  onClick={() => openIssue(null)}
                  data-testid="triage-peek-close"
                >
                  <IconX className="size-4" />
                </Button>
              </div>
            </header>
            <div className="p-4">
              <IssueEditCard issueId={selectedIssueId} />
            </div>
          </>
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center px-6 text-center text-sm text-muted-foreground">
            <IconInbox className="mb-2 size-8 text-muted-foreground/50" />
            <p className="font-medium">Select an issue</p>
            <p className="mt-1 text-xs">
              Use <kbd className="rounded border bg-muted px-1 font-mono">j</kbd>/
              <kbd className="rounded border bg-muted px-1 font-mono">k</kbd> to
              move, <kbd className="rounded border bg-muted px-1 font-mono">Enter</kbd>{" "}
              to open. Press{" "}
              <kbd className="rounded border bg-muted px-1 font-mono">?</kbd> for the
              full cheatsheet.
            </p>
          </div>
        )}
      </aside>

      {/* ⌘K command palette — slice-2 scope: jump-to (smart view),
          view switch, and apply-to-selection. Lightweight: a list
          of commands gated by query substring with keyboard cursor
          navigation deferred to a future iteration (clicking and
          Enter on the focused row already cover the slice-2 goal). */}
      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        commands={[
          {
            id: "view:mine",
            label: "Go to My queue",
            run: () => goToView("mine"),
          },
          {
            id: "view:untriaged",
            label: "Go to Untriaged",
            run: () => goToView("untriaged"),
          },
          {
            id: "view:snoozed",
            label: "Go to Snoozed",
            run: () => goToView("snoozed"),
          },
          {
            id: "view:all",
            label: "Go to All issues",
            run: () => goToView("all"),
          },
          ...(selected.size > 0
            ? [
                {
                  id: "apply:done",
                  label: `Mark ${selected.size} done`,
                  run: () => {
                    const ids = Array.from(selected);
                    bulkInbox.mutate({ issueIds: ids, op: "done_all" });
                    clearSelected();
                  },
                },
                {
                  id: "apply:snooze",
                  label: `Snooze ${selected.size} for 1 day`,
                  run: () => {
                    const ids = Array.from(selected);
                    const wake = new Date(
                      Date.now() + 24 * 60 * 60 * 1000,
                    ).toISOString();
                    bulkInbox.mutate({
                      issueIds: ids,
                      op: "snooze_all",
                      snoozedUntil: wake,
                    });
                    clearSelected();
                  },
                },
                {
                  id: "apply:restore",
                  label: `Restore ${selected.size} to inbox`,
                  run: () => {
                    const ids = Array.from(selected);
                    bulkInbox.mutate({ issueIds: ids, op: "inbox_all" });
                    clearSelected();
                  },
                },
              ]
            : []),
          ...sortedRows.slice(0, 8).map((r) => ({
            id: `jump:${r.id}`,
            label: `#${r.number} · ${r.title}`,
            run: () => openIssue(r.id),
          })),
        ]}
      />

      {/* `?` cheatsheet — same content as the legacy issues page. */}
      <Dialog open={helpOpen} onOpenChange={setHelpOpen}>
        <DialogContent className="sm:max-w-md" data-testid="triage-help-dialog">
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
            <dt className="font-mono text-muted-foreground">g d</dt>
            <dd>Toggle Due column</dd>
            <dt className="font-mono text-muted-foreground">?</dt>
            <dd>Toggle this help</dd>
          </dl>
        </DialogContent>
      </Dialog>
    </div>
  );
}

/**
 * Compact relative-time renderer for list rows. Keeps the row
 * height fixed — `1h`, `3d`, `Apr 12` instead of full timestamps.
 * The full timestamp is exposed via the row title attribute in
 * the peek panel.
 */
/**
 * Date label for the `Due` column — short, comparable, locale-
 * aware. Past-due rows render as e.g. `-3d`; future ones as
 * `Apr 12` so the day is unambiguous across week boundaries.
 */
function formatDueLabel(iso: string): string {
  const t = new Date(iso).getTime();
  if (!Number.isFinite(t)) return "";
  const days = Math.round((t - Date.now()) / (24 * 60 * 60 * 1000));
  if (days < 0) return `${days}d`;
  if (days === 0) return "today";
  if (days <= 7) return `${days}d`;
  return new Date(iso).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

function formatRelative(iso: string): string {
  const t = new Date(iso).getTime();
  if (!Number.isFinite(t)) return "";
  const secs = Math.max(0, Math.floor((Date.now() - t) / 1000));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h`;
  const days = Math.floor(hrs / 24);
  if (days < 7) return `${days}d`;
  return new Date(iso).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

interface PaletteCommand {
  id: string;
  label: string;
  run: () => void;
}

/**
 * Minimal ⌘K palette — substring filter over the supplied command
 * list, ↑/↓ moves the cursor, Enter runs, Esc closes. Deliberately
 * built without a third-party (cmdk) so it can land in slice 2
 * without bumping the dependency surface; if the operator wants
 * fuzzy ranking later, swap the filter line.
 */
function CommandPalette({
  open,
  onClose,
  commands,
}: {
  open: boolean;
  onClose: () => void;
  commands: PaletteCommand[];
}): JSX.Element {
  const [q, setQ] = useState("");
  const [cursor, setCursor] = useState(0);
  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    if (!needle) return commands;
    return commands.filter((c) => c.label.toLowerCase().includes(needle));
  }, [commands, q]);
  useEffect(() => {
    if (open) {
      setQ("");
      setCursor(0);
    }
  }, [open]);
  useEffect(() => {
    setCursor((c) => Math.min(c, Math.max(0, filtered.length - 1)));
  }, [filtered.length]);
  return (
    <Dialog open={open} onOpenChange={(v) => (v ? undefined : onClose())}>
      <DialogContent
        className="sm:max-w-lg"
        data-testid="triage-palette"
        aria-label="Command palette"
      >
        <DialogHeader>
          <DialogTitle className="sr-only">Command palette</DialogTitle>
          <DialogDescription className="sr-only">
            Jump to a view, switch sections, or apply an action to the
            current selection.
          </DialogDescription>
        </DialogHeader>
        <input
          autoFocus
          data-testid="triage-palette-input"
          value={q}
          placeholder="Type a command…"
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              setCursor((c) => Math.min(filtered.length - 1, c + 1));
              e.preventDefault();
            } else if (e.key === "ArrowUp") {
              setCursor((c) => Math.max(0, c - 1));
              e.preventDefault();
            } else if (e.key === "Enter") {
              const cmd = filtered[cursor];
              if (cmd) {
                cmd.run();
                onClose();
              }
              e.preventDefault();
            }
          }}
          className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary"
        />
        <ul
          className="max-h-72 overflow-y-auto"
          data-testid="triage-palette-list"
        >
          {filtered.length === 0 && (
            <li className="px-3 py-6 text-center text-xs text-muted-foreground">
              No matches.
            </li>
          )}
          {filtered.map((c, i) => (
            <li key={c.id}>
              <button
                type="button"
                data-testid="triage-palette-item"
                data-cursor={i === cursor ? "true" : undefined}
                onClick={() => {
                  c.run();
                  onClose();
                }}
                onMouseEnter={() => setCursor(i)}
                className={`w-full truncate rounded-md px-3 py-2 text-left text-sm ${
                  i === cursor ? "bg-accent" : "hover:bg-accent/50"
                }`}
              >
                {c.label}
              </button>
            </li>
          ))}
        </ul>
      </DialogContent>
    </Dialog>
  );
}


// ---------------------------------------------------------------------------
// localStorage helpers — used by the splitter widths and the `Due`
// column toggle. The pref keys live under the `dp:triage:*` prefix so
// a future "reset triage layout" affordance can wipe them in one go.
// ---------------------------------------------------------------------------

const PREF_PREFIX = "dp:triage:";

function readBoolPref(key: string, fallback: boolean): boolean {
  if (typeof window === "undefined") return fallback;
  try {
    const v = window.localStorage.getItem(PREF_PREFIX + key);
    if (v === null) return fallback;
    return v === "1" || v === "true";
  } catch {
    return fallback;
  }
}

function writeBoolPref(key: string, value: boolean): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(PREF_PREFIX + key, value ? "1" : "0");
  } catch {
    /* quota / disabled — pref is in-memory only this session */
  }
}

function readNumberPref(key: string, fallback: number): number {
  if (typeof window === "undefined") return fallback;
  try {
    const v = window.localStorage.getItem(PREF_PREFIX + key);
    if (v === null) return fallback;
    const n = Number.parseFloat(v);
    return Number.isFinite(n) ? n : fallback;
  } catch {
    return fallback;
  }
}

function writeNumberPref(key: string, value: number): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(PREF_PREFIX + key, String(value));
  } catch {
    /* see writeBoolPref */
  }
}

/**
 * Vertical drag-handle that resizes the pane to its left (or right —
 * see `side`). Pointer-events-only so it works under mouse and touch
 * without dragging in a third-party splitter library.
 */
function SplitHandle({
  side,
  width,
  setWidth,
  min,
  max,
}: {
  side: "left" | "right";
  width: number;
  setWidth: (n: number) => void;
  min: number;
  max: number;
}): JSX.Element {
  const draggingRef = useRef(false);
  const startXRef = useRef(0);
  const startWRef = useRef(width);
  const onDown = (e: React.PointerEvent<HTMLDivElement>) => {
    draggingRef.current = true;
    startXRef.current = e.clientX;
    startWRef.current = width;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  };
  const onMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return;
    const dx = e.clientX - startXRef.current;
    const signed = side === "left" ? dx : -dx;
    const next = Math.min(max, Math.max(min, startWRef.current + signed));
    setWidth(next);
  };
  const onUp = (e: React.PointerEvent<HTMLDivElement>) => {
    draggingRef.current = false;
    (e.target as HTMLElement).releasePointerCapture(e.pointerId);
  };
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      data-testid={`triage-split-${side}`}
      className="w-1 cursor-col-resize bg-transparent hover:bg-primary/40 transition-colors"
      onPointerDown={onDown}
      onPointerMove={onMove}
      onPointerUp={onUp}
      onPointerCancel={onUp}
    />
  );
}

