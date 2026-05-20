/**
 * Filters card body for the repo-activity report. Composes:
 *
 *   - Orgs   — MultiSelect.
 *   - Repos  — MultiSelect, scoped to repos owned by the selected orgs.
 *   - Kinds  — MultiSelect of `ACTIVITY_KINDS`.
 *   - Window — shared `WindowPicker`.
 */

import { useId, useMemo } from "react";

import { Label } from "@/components/ui/label";

import type { OrgDto, RepoSummaryDto } from "../../api/client.js";

import { ACTIVITY_KINDS } from "../activity-types.js";
import { MultiSelect } from "../leaderboard/multi-select.jsx";
import {
  FILTER_GRID_CLASS,
  WindowPicker,
  type WindowState,
} from "../window-picker.jsx";

export interface RepoActivityFiltersProps {
  orgs: ReadonlyArray<OrgDto>;
  repos: ReadonlyArray<RepoSummaryDto>;
  selectedOrgIds: ReadonlyArray<string>;
  selectedRepoIds: ReadonlyArray<string>;
  selectedKinds: ReadonlyArray<string>;
  onOrgsChange: (ids: string[]) => void;
  onReposChange: (ids: string[]) => void;
  onKindsChange: (kinds: string[]) => void;
  windowState: WindowState;
  onWindowChange: (state: WindowState) => void;
  orgsLoading: boolean;
  reposLoading: boolean;
}

export function RepoActivityFilters({
  orgs,
  repos,
  selectedOrgIds,
  selectedRepoIds,
  selectedKinds,
  onOrgsChange,
  onReposChange,
  onKindsChange,
  windowState,
  onWindowChange,
  orgsLoading,
  reposLoading,
}: RepoActivityFiltersProps): JSX.Element {
  const orgId = useId();
  const repoId = useId();
  const kindId = useId();

  const orgOptions = useMemo(
    () =>
      orgs.map((o) => ({
        value: o.id,
        label: o.name ?? o.login,
        hint: o.name ? o.login : undefined,
      })),
    [orgs],
  );

  // Limit the repo picker to repos owned by the selected orgs — and
  // when no org is selected, show every repo (the empty state on the
  // page already prompts the user to pick an org first).
  const repoOptions = useMemo(() => {
    const orgFilter =
      selectedOrgIds.length > 0 ? new Set(selectedOrgIds) : null;
    return repos
      .filter((r) => (orgFilter ? orgFilter.has(r.org_id) : true))
      .map((r) => ({
        value: r.id,
        label: r.slug,
        hint: undefined,
      }));
  }, [repos, selectedOrgIds]);

  const kindOptions = useMemo(
    () => ACTIVITY_KINDS.map((k) => ({ value: k.key, label: k.label })),
    [],
  );

  return (
    <div className={FILTER_GRID_CLASS}>
      <div className="grid gap-1.5">
        <Label htmlFor={orgId}>Orgs</Label>
        <MultiSelect
          id={orgId}
          data-testid="repo-activity-org-select"
          placeholder={orgsLoading ? "Loading orgs…" : "Select orgs"}
          options={orgOptions}
          value={selectedOrgIds}
          onChange={onOrgsChange}
          disabled={orgsLoading || orgs.length === 0}
        />
      </div>

      <div className="grid gap-1.5">
        <Label htmlFor={repoId}>Repos</Label>
        <MultiSelect
          id={repoId}
          data-testid="repo-activity-repo-select"
          placeholder={reposLoading ? "Loading repos…" : "All repos"}
          options={repoOptions}
          value={selectedRepoIds}
          onChange={onReposChange}
          disabled={reposLoading || repoOptions.length === 0}
          summary={(sel) =>
            sel.length === 0
              ? "All repos"
              : `${sel.length} repo${sel.length === 1 ? "" : "s"}`
          }
        />
      </div>

      <div className="grid gap-1.5">
        <Label htmlFor={kindId}>Activity types</Label>
        <MultiSelect
          id={kindId}
          data-testid="repo-activity-kind-select"
          placeholder="All activity"
          options={kindOptions}
          value={selectedKinds}
          onChange={onKindsChange}
          summary={(sel) =>
            sel.length === kindOptions.length
              ? "All activity"
              : `${sel.length} kind${sel.length === 1 ? "" : "s"}`
          }
        />
      </div>

      <WindowPicker value={windowState} onChange={onWindowChange} />
    </div>
  );
}
