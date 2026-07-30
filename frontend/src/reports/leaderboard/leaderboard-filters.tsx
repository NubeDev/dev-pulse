/**
 * Filters card body for the leaderboard. Composes:
 *
 *   - Orgs    — MultiSelect (one user can belong to many orgs).
 *   - Users   — MultiSelect, filtered to members of the selected orgs.
 *   - Kinds   — MultiSelect of `ACTIVITY_KINDS`.
 *   - Window  — shared `WindowPicker` (preset + TZ + anchor).
 */

import { useId, useMemo } from "react";

import { Label } from "@/components/ui/label";

import type { OrgDto, UserDto } from "../../api/client.js";

import { ACTIVITY_KINDS } from "../activity-types.js";
import {
  FILTER_GRID_CLASS,
  WindowPicker,
  type WindowState,
} from "../window-picker.jsx";

import { MultiSelect } from "../../components/multi-select.jsx";

export interface LeaderboardFiltersProps {
  orgs: ReadonlyArray<OrgDto>;
  users: ReadonlyArray<UserDto>;
  selectedOrgIds: ReadonlyArray<string>;
  selectedUserIds: ReadonlyArray<string>;
  selectedKinds: ReadonlyArray<string>;
  onOrgsChange: (ids: string[]) => void;
  onUsersChange: (ids: string[]) => void;
  onKindsChange: (kinds: string[]) => void;
  windowState: WindowState;
  onWindowChange: (state: WindowState) => void;
  orgsLoading: boolean;
  usersLoading: boolean;
}

export function LeaderboardFilters({
  orgs,
  users,
  selectedOrgIds,
  selectedUserIds,
  selectedKinds,
  onOrgsChange,
  onUsersChange,
  onKindsChange,
  windowState,
  onWindowChange,
  orgsLoading,
  usersLoading,
}: LeaderboardFiltersProps): JSX.Element {
  const orgId = useId();
  const userId = useId();
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

  const userOptions = useMemo(
    () =>
      users.map((u) => ({
        value: u.id,
        label: u.name ?? u.login,
        hint: u.name ? u.login : undefined,
      })),
    [users],
  );

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
          data-testid="leaderboard-org-select"
          placeholder={orgsLoading ? "Loading orgs…" : "Select orgs"}
          options={orgOptions}
          value={selectedOrgIds}
          onChange={onOrgsChange}
          disabled={orgsLoading || orgs.length === 0}
        />
      </div>

      <div className="grid gap-1.5">
        <Label htmlFor={userId}>Users</Label>
        <MultiSelect
          id={userId}
          data-testid="leaderboard-user-select"
          placeholder={usersLoading ? "Loading users…" : "All users"}
          options={userOptions}
          value={selectedUserIds}
          onChange={onUsersChange}
          disabled={usersLoading || users.length === 0}
          summary={(sel) =>
            sel.length === users.length
              ? "All users"
              : `${sel.length} user${sel.length === 1 ? "" : "s"}`
          }
        />
      </div>

      <div className="grid gap-1.5">
        <Label htmlFor={kindId}>Activity types</Label>
        <MultiSelect
          id={kindId}
          data-testid="leaderboard-kind-select"
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
