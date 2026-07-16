/**
 * `UserPicker` / `UserLoginsPicker` — pickers for GitHub users
 * scoped to a single org. Source of truth is `GET /users?org_id=…`.
 *
 * Two variants because the two consumers store users differently:
 *
 *   * `UserPicker` — single-value, returns the internal
 *     `dp_users.id` UUID (or `null` to clear). Used for the
 *     project `lead_user_id` field.
 *   * `UserLoginsPicker` — multi-value, returns GitHub
 *     `login` strings. Used for the issue `assignees` field
 *     (which is stored as GitHub logins and round-trips to the
 *     GitHub Issues API).
 *
 * Both gracefully degrade when `orgId` is undefined (renders a
 * disabled trigger explaining the constraint) and when the users
 * query is still loading.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check, ChevronDown, Search, X } from "lucide-react";
import { Popover as PopoverPrimitive } from "radix-ui";

import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { api } from "../api/client.js";
import type { UserDto } from "../api/client.js";
import { SearchableSelect } from "./searchable-select.js";
import type { SearchableSelectOption } from "./searchable-select.js";

function useOrgUsers(orgId: string | undefined) {
  return useQuery({
    queryKey: ["users", orgId ?? "__none__"],
    queryFn: () => api.listUsers(orgId),
    enabled: orgId !== undefined,
    staleTime: 60_000,
  });
}

function userLabel(u: UserDto): string {
  return u.name && u.name.trim().length > 0 ? u.name : u.login;
}

function userHint(u: UserDto): string | undefined {
  return u.name && u.name.trim().length > 0 ? `@${u.login}` : undefined;
}

// ---------------------------------------------------------------------------
// Single-value — project lead (UUID).
// ---------------------------------------------------------------------------

export interface UserPickerProps {
  orgId: string | undefined;
  /** Internal `dp_users.id` UUID, or `null` when unassigned. */
  value: string | null;
  /** Receives the new UUID, or `null` to clear. */
  onChange: (next: string | null) => void;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  id?: string;
  "data-testid"?: string;
}

const CLEAR_SENTINEL = "__clear__";

export function UserPicker({
  orgId,
  value,
  onChange,
  placeholder = "Unassigned",
  disabled,
  className,
  id,
  ...rest
}: UserPickerProps): JSX.Element {
  const users = useOrgUsers(orgId);

  const options: ReadonlyArray<SearchableSelectOption> = useMemo(() => {
    const base: SearchableSelectOption[] = [];
    // The clear sentinel sits at the top so it's always reachable
    // even with a long member list. Disabled when nothing is set.
    if (value !== null) {
      base.push({ value: CLEAR_SENTINEL, label: "— Unassigned —" });
    }
    for (const u of users.data ?? []) {
      base.push({ value: u.id, label: userLabel(u), hint: userHint(u) });
    }
    return base;
  }, [users.data, value]);

  return (
    <SearchableSelect
      id={id}
      data-testid={rest["data-testid"]}
      className={className}
      disabled={disabled || orgId === undefined || users.isLoading}
      placeholder={
        orgId === undefined
          ? "No org selected"
          : users.isLoading
            ? "Loading users…"
            : placeholder
      }
      options={options}
      value={value}
      onChange={(next) => {
        if (next === CLEAR_SENTINEL) onChange(null);
        else onChange(next);
      }}
      searchPlaceholder="Search members…"
      // A failed /users fetch used to fall through `?? []` and render
      // as the generic "No options available." — indistinguishable
      // from an org with no members, which is what made this bug so
      // hard to place. Name the failure instead.
      emptyLabel={
        users.isError ? "Failed to load members." : "No members available."
      }
    />
  );
}

// ---------------------------------------------------------------------------
// Multi-value — issue assignees (GitHub logins).
// ---------------------------------------------------------------------------

export interface UserLoginsPickerProps {
  orgId: string | undefined;
  /** GitHub `login` strings currently assigned. */
  value: ReadonlyArray<string>;
  onChange: (next: string[]) => void;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  id?: string;
  "data-testid"?: string;
  /** Optional cap (GitHub allows up to 10 assignees per issue). */
  maxSelected?: number;
}

export function UserLoginsPicker({
  orgId,
  value,
  onChange,
  placeholder = "No assignees",
  disabled,
  className,
  id,
  maxSelected = 10,
  ...rest
}: UserLoginsPickerProps): JSX.Element {
  const users = useOrgUsers(orgId);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");

  // Resolve current logins to UserDto (when known) so we can render
  // display names; fall back to the raw login when the user isn't
  // in the org list (former member, transferred repo, etc.).
  const known = users.data ?? [];
  const byLogin = useMemo(() => {
    const m = new Map<string, UserDto>();
    for (const u of known) m.set(u.login, u);
    return m;
  }, [known]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return known;
    return known.filter(
      (u) =>
        u.login.toLowerCase().includes(q) ||
        (u.name?.toLowerCase().includes(q) ?? false),
    );
  }, [known, query]);

  const isSelected = (login: string): boolean => value.includes(login);

  const toggle = (login: string): void => {
    if (isSelected(login)) {
      onChange(value.filter((l) => l !== login));
    } else {
      if (value.length >= maxSelected) return;
      onChange([...value, login]);
    }
  };

  const trigger = (
    <PopoverPrimitive.Trigger
      id={id}
      disabled={disabled || orgId === undefined || users.isLoading}
      data-testid={rest["data-testid"]}
      aria-haspopup="listbox"
      className={cn(
        "inline-flex min-h-9 w-full shrink-0 items-center justify-between gap-2 rounded-md border bg-background px-3 py-1.5 text-sm font-normal shadow-xs outline-none transition-all",
        "hover:bg-accent hover:text-accent-foreground dark:border-input dark:bg-input/30 dark:hover:bg-input/50",
        "focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50",
        "disabled:pointer-events-none disabled:opacity-50",
        className,
      )}
    >
      <span className="flex flex-1 flex-wrap items-center gap-1">
        {value.length === 0 ? (
          <span className="text-muted-foreground">
            {orgId === undefined
              ? "No org selected"
              : users.isLoading
                ? "Loading users…"
                : placeholder}
          </span>
        ) : (
          value.map((login) => {
            const u = byLogin.get(login);
            const label = u ? userLabel(u) : login;
            return (
              <span
                key={login}
                className="inline-flex items-center gap-1 rounded-sm bg-muted px-1.5 py-0.5 text-xs"
                title={u ? `@${login}` : login}
              >
                {label}
                <span
                  role="button"
                  tabIndex={0}
                  aria-label={`Remove ${login}`}
                  className="cursor-pointer rounded-sm p-0.5 text-muted-foreground hover:bg-background hover:text-foreground"
                  onPointerDown={(e) => {
                    // Avoid the parent Trigger toggling the popover.
                    e.preventDefault();
                    e.stopPropagation();
                    onChange(value.filter((l) => l !== login));
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      e.stopPropagation();
                      onChange(value.filter((l) => l !== login));
                    }
                  }}
                >
                  <X className="h-3 w-3" aria-hidden />
                </span>
              </span>
            );
          })
        )}
      </span>
      <ChevronDown className="size-4 shrink-0 opacity-50" aria-hidden />
    </PopoverPrimitive.Trigger>
  );

  return (
    <PopoverPrimitive.Root
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) setQuery("");
      }}
    >
      {trigger}
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          align="start"
          sideOffset={4}
          className={cn(
            "z-50 rounded-md border bg-popover p-1 text-popover-foreground shadow-md",
            "min-w-(--radix-popover-trigger-width) w-(--radix-popover-trigger-width)",
          )}
          style={{ minWidth: "16rem" }}
        >
          <div className="relative px-1 pb-1 pt-1">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
              aria-hidden
            />
            <Input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search members…"
              className="h-8 pl-7 text-xs"
              aria-label="Search members"
            />
          </div>
          <div className="-mx-1 my-1 h-px bg-border" />
          <div
            role="listbox"
            aria-multiselectable
            className="overflow-y-auto"
            style={{ maxHeight: "18rem" }}
          >
            {known.length === 0 ? (
              <p className="px-2 py-3 text-xs text-muted-foreground">
                {users.isError
                  ? "Failed to load members."
                  : "No members available."}
              </p>
            ) : visible.length === 0 ? (
              <p className="px-2 py-3 text-xs text-muted-foreground">
                No matches for &ldquo;{query}&rdquo;.
              </p>
            ) : (
              visible.map((u) => {
                const checked = isSelected(u.login);
                const atCap = !checked && value.length >= maxSelected;
                return (
                  <button
                    key={u.id}
                    type="button"
                    role="option"
                    aria-selected={checked}
                    disabled={atCap}
                    onClick={() => toggle(u.login)}
                    className={cn(
                      "flex w-full cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none",
                      "hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground",
                      atCap && "opacity-50",
                    )}
                  >
                    <Check
                      className={cn(
                        "size-3.5 shrink-0",
                        checked ? "opacity-100" : "opacity-0",
                      )}
                      aria-hidden
                    />
                    <span className="flex flex-1 items-center justify-between gap-2 truncate">
                      <span className="truncate">{userLabel(u)}</span>
                      <span className="truncate text-xs text-muted-foreground">
                        @{u.login}
                      </span>
                    </span>
                  </button>
                );
              })
            )}
            {value.length >= maxSelected && (
              <p className="border-t px-2 py-1.5 text-[10px] text-muted-foreground">
                Maximum {maxSelected} assignees — remove one to add another.
              </p>
            )}
          </div>
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  );
}
