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
 *
 * Surfaces use shadcn `Alert` — the error path is `variant="destructive"`,
 * the result panel is the default variant. The `data-testid` lands on
 * the Alert root so the smoke tests still see `refresh-result`.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert, AlertDescription, AlertTitle } from "@nube/starter-ui-kit/components/alert";
import { Badge } from "@nube/starter-ui-kit/components/badge";
import { Button } from "@nube/starter-ui-kit/components/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";
import { cn } from "@nube/starter-ui-kit/lib/utils";
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
      <CardContent className="grid gap-4">
        <div className="grid max-w-md gap-1">
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

        <div className="flex flex-wrap items-center gap-3">
          <Button
            data-testid="refresh-trigger"
            disabled={refresh.isPending}
            onClick={trigger}
          >
            {refresh.isPending ? "Refreshing…" : "Trigger refresh"}
          </Button>
          <span className="text-sm text-muted-foreground">
            Scope:{" "}
            <code data-testid="refresh-scope">
              {orgId === ALL_ORGS ? "all orgs" : selectedOrg?.login ?? orgId.slice(0, 8)}
            </code>
          </span>
        </div>

        {error ? (
          <Alert variant="destructive" data-testid="refresh-error">
            <AlertTitle>Refresh failed</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}

        {lastResult ? (
          <Alert
            data-testid="refresh-result"
            data-ran={lastResult.ran}
            aria-live="polite"
          >
            {lastResult.ran ? (
              <>
                <AlertTitle>Refresh complete.</AlertTitle>
                <AlertDescription>
                  <div className="flex flex-wrap gap-2 pt-1">
                    <Badge variant="outline" data-testid="refresh-items">
                      {lastResult.items} items
                    </Badge>
                    <Badge
                      variant="outline"
                      data-testid="refresh-errors"
                      className={cn(
                        "border",
                        lastResult.errors > 0 &&
                          "border-red-500 text-red-600 dark:text-red-400",
                      )}
                    >
                      {lastResult.errors} errors
                    </Badge>
                    {lastResult.partial ? (
                      <Badge
                        variant="outline"
                        data-testid="refresh-partial"
                        className="border-amber-500 text-amber-600 dark:text-amber-400"
                      >
                        Partial
                      </Badge>
                    ) : null}
                  </div>
                </AlertDescription>
              </>
            ) : (
              <>
                <AlertTitle>No-op.</AlertTitle>
                <AlertDescription>
                  The reconciler was already running or recently completed; nothing to do.
                </AlertDescription>
              </>
            )}
          </Alert>
        ) : null}
      </CardContent>
    </Card>
  );
}
