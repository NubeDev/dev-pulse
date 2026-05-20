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
 * Layout (stage 4 visual rewrite): PageHeading lockup at top, then a
 * single form Card holding the org scope `Select`, the trigger
 * Button, and one always-mounted status `Alert` that walks through
 * four states (idle / loading + spinner / success + check /
 * destructive). The `data-testid="refresh-result"` hook lands on the
 * Alert root in the success branch so the smoke tests still see it.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";

import { api } from "../api/client.js";
import type { RefreshResponse } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { MOCK_ORGS, USE_MOCK, mockRefresh } from "./mocks.js";

/** Sentinel value for the "all orgs" option in the `<Select>`.  We use
 *  a sentinel rather than `undefined` because Radix's `<SelectItem>`
 *  refuses an empty string. */
const ALL_ORGS = "__all__";

/** Tiny inline check icon used in the success Alert. Avoids pulling
 *  a new icon dep just for one glyph. Sized to match Alert's left
 *  icon slot. */
function CheckIcon(): JSX.Element {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.25"
      strokeLinecap="round"
      strokeLinejoin="round"
      className="size-4 text-emerald-600 dark:text-emerald-400"
    >
      <path d="M20 6 9 17l-5-5" />
    </svg>
  );
}

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
    <div className="grid gap-6">
      <PageHeading
        title="Refresh trigger"
        description={
          <>
            <code className="font-mono text-xs">POST /admin/refresh</code> ·
            operator-triggered reconciler tick. Narrow to one org with the
            selector below, or leave on "All orgs" for a full sweep.
          </>
        }
      />

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-medium">Trigger reconciler</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          <div className="grid max-w-md gap-1.5">
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
              <code className="font-mono text-xs" data-testid="refresh-scope">
                {orgId === ALL_ORGS ? "all orgs" : selectedOrg?.login ?? orgId.slice(0, 8)}
              </code>
            </span>
          </div>

          {error ? (
            <Alert variant="destructive" data-testid="refresh-error">
              <AlertTitle>Refresh failed</AlertTitle>
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : refresh.isPending ? (
            <Alert
              data-testid="refresh-status"
              data-kind="loading"
              aria-live="polite"
            >
              <Spinner className="text-muted-foreground" />
              <AlertTitle>Refreshing…</AlertTitle>
              <AlertDescription>
                Asking dp-rest to reconcile the selected scope.
              </AlertDescription>
            </Alert>
          ) : lastResult ? (
            <Alert
              data-testid="refresh-result"
              data-ran={lastResult.ran}
              aria-live="polite"
            >
              {lastResult.ran ? (
                <>
                  <CheckIcon />
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
                  <CheckIcon />
                  <AlertTitle>No-op.</AlertTitle>
                  <AlertDescription>
                    The reconciler was already running or recently completed;
                    nothing to do.
                  </AlertDescription>
                </>
              )}
            </Alert>
          ) : (
            <Alert data-testid="refresh-status" data-kind="idle">
              <AlertTitle>Ready</AlertTitle>
              <AlertDescription>
                Pick a scope and trigger the reconciler to see results here.
              </AlertDescription>
            </Alert>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
