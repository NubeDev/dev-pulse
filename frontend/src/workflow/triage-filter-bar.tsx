/**
 * `TriageFilterBar` — the multi-select filter row above the triage
 * list.
 *
 * Four pickers (user, project, repo, label) plus a state select.
 * Every one is **OR within itself, AND across pickers**: picking
 * Alice + Bob shows work assigned to either of them; adding
 * project "Gen-02" narrows that to their Gen-02 work.
 *
 * All selections live in the URL (`?users=…&projects=…`), so a
 * filtered queue is copy-pasteable and survives reload / back —
 * same contract the label filter already had.
 *
 * The option lists come from the same endpoints the rest of the app
 * uses (`GET /users`, `GET /projects`, `GET /repos`); labels are
 * derived from the rows currently on screen because there is no
 * label-directory endpoint.
 */

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { X } from "lucide-react";

import { MultiSelect } from "@/components/multi-select";
import type { MultiSelectOption } from "@/components/multi-select";
import { api } from "../api/client.js";
import type { IssueListItem, UserDto } from "../api/client.js";
import { useProjectList } from "../projects/use-projects-data.js";

/** The complete filter state the triage page reads off the route. */
export interface TriageFilters {
  users: string[];
  projects: string[];
  repos: string[];
  labels: string[];
  state: "open" | "closed" | "all";
}

export interface TriageFilterBarProps {
  filters: TriageFilters;
  /** Replace one filter key. The page maps this onto the route. */
  onChange: <K extends keyof TriageFilters>(
    key: K,
    value: TriageFilters[K],
  ) => void;
  /** Drop every filter at once. */
  onClear: () => void;
  /** Rows currently rendered — the label options are derived from
   *  these (no label-directory endpoint exists). */
  rows: ReadonlyArray<IssueListItem>;
}

function userOption(u: UserDto): MultiSelectOption {
  const named = u.name && u.name.trim().length > 0;
  return {
    // The backend matches assignees on GitHub login, not user id.
    value: u.login,
    label: named ? (u.name as string) : u.login,
    ...(named ? { hint: `@${u.login}` } : {}),
  };
}

export function TriageFilterBar({
  filters,
  onChange,
  onClear,
  rows,
}: TriageFilterBarProps): JSX.Element {
  const usersQ = useQuery({
    queryKey: ["users", "__all__"],
    queryFn: () => api.listUsers(),
    staleTime: 60_000,
  });

  // Active projects only — archived ones would just be noise in a
  // triage filter.
  const projectsQ = useProjectList({ status: "active", limit: 200 });

  const userOptions = useMemo<MultiSelectOption[]>(
    () => (usersQ.data ?? []).map(userOption),
    [usersQ.data],
  );

  const projectOptions = useMemo<MultiSelectOption[]>(
    () =>
      (projectsQ.data?.rows ?? []).map((p) => ({
        value: p.id,
        label: p.name,
      })),
    [projectsQ.data],
  );

  // Repo + label options are derived from the rows on screen. That
  // keeps them honest (you can only filter to something that's
  // actually there) and costs no extra round-trip.
  const repoOptions = useMemo<MultiSelectOption[]>(() => {
    const seen = new Map<string, string>();
    for (const r of rows) {
      if (r.repo_slug && !seen.has(r.repo_id)) seen.set(r.repo_id, r.repo_slug);
    }
    return [...seen].map(([value, label]) => ({ value, label }));
  }, [rows]);

  const labelOptions = useMemo<MultiSelectOption[]>(() => {
    const seen = new Set<string>();
    for (const r of rows) for (const l of r.labels) seen.add(l);
    return [...seen].sort().map((l) => ({ value: l, label: l }));
  }, [rows]);

  const activeCount =
    filters.users.length +
    filters.projects.length +
    filters.repos.length +
    filters.labels.length +
    (filters.state === "open" ? 0 : 1);

  return (
    <div
      className="flex flex-wrap items-center gap-2 border-b px-4 py-2"
      data-testid="triage-filter-bar"
    >
      <MultiSelect
        options={userOptions}
        value={filters.users}
        onChange={(next) => onChange("users", next)}
        placeholder="Assignee"
        searchable
        searchPlaceholder="Search people…"
        className="w-44"
        data-testid="triage-filter-users"
        summary={(sel) =>
          sel.length === 1 ? (sel[0]?.label ?? "") : `${sel.length} people`
        }
      />

      <MultiSelect
        options={projectOptions}
        value={filters.projects}
        onChange={(next) => onChange("projects", next)}
        placeholder="Project"
        searchable
        searchPlaceholder="Search projects…"
        className="w-44"
        data-testid="triage-filter-projects"
        summary={(sel) =>
          sel.length === 1 ? (sel[0]?.label ?? "") : `${sel.length} projects`
        }
      />

      <MultiSelect
        options={repoOptions}
        value={filters.repos}
        onChange={(next) => onChange("repos", next)}
        placeholder="Repo"
        searchable
        searchPlaceholder="Search repos…"
        className="w-44"
        data-testid="triage-filter-repos"
        summary={(sel) =>
          sel.length === 1 ? (sel[0]?.label ?? "") : `${sel.length} repos`
        }
      />

      <MultiSelect
        options={labelOptions}
        value={filters.labels}
        onChange={(next) => onChange("labels", next)}
        placeholder="Label"
        searchable
        searchPlaceholder="Search labels…"
        className="w-40"
        data-testid="triage-filter-labels"
        summary={(sel) =>
          sel.length === 1 ? (sel[0]?.label ?? "") : `${sel.length} labels`
        }
      />

      <select
        className="h-9 rounded-md border bg-background px-2 text-sm"
        value={filters.state}
        onChange={(e) =>
          onChange("state", e.target.value as TriageFilters["state"])
        }
        aria-label="State filter"
        data-testid="triage-filter-state"
      >
        <option value="open">Open</option>
        <option value="closed">Closed</option>
        <option value="all">All states</option>
      </select>

      {activeCount > 0 && (
        <button
          type="button"
          onClick={onClear}
          className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
          data-testid="triage-filter-clear"
        >
          <X className="size-3" />
          Clear {activeCount}
        </button>
      )}
    </div>
  );
}
