/**
 * Admin · Refresh page — `POST /admin/refresh` trigger with org scope.
 *
 * The reconciler usually ticks on its own schedule; this button is
 * the operator's escape hatch when they need to force a refresh
 * immediately (e.g. after a manual GitHub change that webhooks
 * missed). dp-rest's `POST /admin/refresh` accepts optional
 * `?org_id=…` to narrow to one org (`repo_id` is also supported but
 * we don't expose it from the UI — repo-level refresh is a CLI move).
 *
 * Renders the previous outcome (items / errors / partial) below the
 * button so the operator gets immediate confirmation; if the
 * reconciler short-circuited (already running, debounced) the
 * response is `{ ran: false }` and we say so.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@nube/starter-ui-kit/components/badge";
import { Button } from "@nube/starter-ui-kit/components/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import { Label } from "@nube/starter-ui-kit/components/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@nube/starter-ui-kit/components/select";

import { api } from "../api/client.js";
import type { RefreshResponse } from "../api/client.js";
import { MOCK_ORGS, USE_MOCK, mockRefresh } from "./mocks.js";

/** Sentinel value for the "all orgs" option in the `<Select>`.  We use
 *  a sentinel rather than `undefined` because Radix's `<SelectItem>`
 *  refuses an empty string. */
const ALL_ORGS = "__all__";

export function RefreshPage(): JSX.Element {
  const qc = useQueryClient();
  const [orgId, setOrgId] = useState<string>(ALL_ORGS);
  const [lastResult, setLastResult] = useState<RefreshResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  const orgsQuery = useQuery({
    queryKey: ["orgs"],
    queryFn: () => (USE_MOCK ? Promise.resolve([...MOCK_ORGS]) : api.listOrgs()),
  });
  const orgs = orgsQuery.data ?? [];

  const refresh = useMutation({
    mutationFn: async (opts: { org_id?: string }) => {
      if (USE_MOCK) {
        await new Promise((r) => setTimeout(r, 50));
        return mockRefresh();
      }
      return api.adminRefresh(opts);
    },
    onSuccess: (data) => {
      setLastResult(data);
      setError(null);
      // A successful refresh changes both the run log and the
      // freshness signal — invalidate so the other admin pages
      // re-fetch when the operator navigates to them.
      void qc.invalidateQueries({ queryKey: ["admin-runs"] });
      void qc.invalidateQueries({ queryKey: ["report-freshness"] });
    },
    onError: (err) => {
      setError(err instanceof Error ? err.message : String(err));
    },
  });

  const trigger = () => {
    setError(null);
    setLastResult(null);
    refresh.mutate(orgId === ALL_ORGS ? {} : { org_id: orgId });
  };

  const selectedOrg = orgs.find((o) => o.id === orgId);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Refresh trigger</CardTitle>
        <CardDescription>
          <code>POST /admin/refresh</code> · operator-triggered reconciler tick.
          Narrow to one org with the selector below, or leave on "All orgs" for
          a full sweep.
        </CardDescription>
      </CardHeader>
      <CardContent style={{ display: "grid", gap: "1rem" }}>
        <div style={{ display: "grid", gap: "0.25rem", maxWidth: "24rem" }}>
          <Label htmlFor="refresh-org">Org scope</Label>
          <Select value={orgId} onValueChange={setOrgId}>
            <SelectTrigger id="refresh-org" data-testid="refresh-org">
              <SelectValue placeholder={orgsQuery.isPending ? "Loading orgs…" : "All orgs"} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL_ORGS}>All orgs (full sweep)</SelectItem>
              {orgs.map((o) => (
                <SelectItem key={o.id} value={o.id}>
                  {o.login}{o.name ? ` — ${o.name}` : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div style={{ display: "flex", gap: "0.75rem", alignItems: "center", flexWrap: "wrap" }}>
          <Button
            data-testid="refresh-trigger"
            disabled={refresh.isPending}
            onClick={trigger}
          >
            {refresh.isPending ? "Refreshing…" : "Trigger refresh"}
          </Button>
          <span style={{ fontSize: "0.875rem", color: "var(--muted-foreground)" }}>
            Scope:{" "}
            <code data-testid="refresh-scope">
              {orgId === ALL_ORGS ? "all orgs" : selectedOrg?.login ?? orgId.slice(0, 8)}
            </code>
          </span>
        </div>

        {error ? (
          <p
            data-testid="refresh-error"
            role="alert"
            style={{ color: "oklch(0.5 0.2 25)" }}
          >
            Refresh failed: {error}
          </p>
        ) : null}

        {lastResult ? (
          <div
            data-testid="refresh-result"
            data-ran={lastResult.ran}
            role="status"
            aria-live="polite"
            style={{
              display: "grid",
              gap: "0.5rem",
              padding: "0.875rem 1rem",
              borderRadius: "var(--radius-md, 0.5rem)",
              border: "1px solid var(--border)",
              background: "var(--muted)",
              fontSize: "0.9rem",
            }}
          >
            {lastResult.ran ? (
              <>
                <strong>Refresh complete.</strong>
                <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
                  <Badge variant="outline" data-testid="refresh-items">
                    {lastResult.items} items
                  </Badge>
                  <Badge
                    variant="outline"
                    data-testid="refresh-errors"
                    style={{
                      color: lastResult.errors > 0 ? "oklch(0.5 0.2 25)" : undefined,
                      borderColor:
                        lastResult.errors > 0 ? "oklch(0.5 0.2 25)" : undefined,
                    }}
                  >
                    {lastResult.errors} errors
                  </Badge>
                  {lastResult.partial ? (
                    <Badge
                      variant="outline"
                      data-testid="refresh-partial"
                      style={{
                        color: "oklch(0.62 0.16 80)",
                        borderColor: "oklch(0.62 0.16 80)",
                      }}
                    >
                      Partial
                    </Badge>
                  ) : null}
                </div>
              </>
            ) : (
              <span>
                <strong>No-op.</strong>{" "}
                <span style={{ color: "var(--muted-foreground)" }}>
                  The reconciler was already running or recently completed; nothing to do.
                </span>
              </span>
            )}
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}
