/**
 * Admin · Runs page — paginated `GET /admin/runs` log with auto-refresh.
 *
 * Renders one row per `fetch_runs` record (kind, started, finished,
 * items, errors, partial) with a derived status column (running /
 * partial / failed / clean) so the operator can scan the log without
 * cross-referencing the numeric columns.
 *
 * Pagination is offset-based — `dp-rest` accepts `limit` + `offset`
 * and returns at most `limit` rows; we infer "has next page" from
 * `rows.length === limit` (one extra page reveals an empty table,
 * which is acceptable for an admin tool).
 *
 * Auto-refresh: react-query's `refetchInterval` ticks every 15s so
 * a freshly started reconciler shows up without an explicit reload.
 * The fetch is paused (`refetchIntervalInBackground: false`) when
 * the tab is hidden so the dashboard doesn't poll when nobody is
 * watching.
 *
 * Layout (stage 4 visual rewrite): PageHeading lockup at top, then a
 * results Card holding the shadcn-shaped `Table` primitive (local —
 * the kit doesn't ship one). Status column uses shadcn `Badge`
 * variants (default for "clean" success, secondary for running /
 * partial, destructive for failed) instead of inline border colours.
 */

import { Fragment, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

import { api } from "../api/client.js";
import type { FetchRunDto, ImportRepoResponse } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../components/table.jsx";
import { USE_MOCK, paginateMockRuns } from "./mocks.js";

const PAGE_SIZE = 25;
const REFRESH_MS = 15_000;

type RunStatus = "running" | "partial" | "failed" | "clean";

function statusOf(run: FetchRunDto): RunStatus {
  if (run.finished === null || run.finished === undefined) return "running";
  if (run.partial) return "partial";
  if (run.errors > 0 && run.items === 0) return "failed";
  return "clean";
}

/** Per-status Badge metadata — variant maps onto shadcn's built-in
 *  variants (default / secondary / destructive). "clean" uses the
 *  default (primary) variant tinted toward success, "running" and
 *  "partial" sit on the muted secondary, "failed" on destructive. */
const STATUS_META: Record<
  RunStatus,
  {
    label: string;
    variant: "default" | "secondary" | "destructive";
    className?: string;
  }
> = {
  running: {
    label: "Running",
    variant: "secondary",
    className:
      "bg-blue-500/10 text-blue-700 dark:text-blue-300",
  },
  partial: {
    label: "Partial",
    variant: "secondary",
    className:
      "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  },
  failed: {
    label: "Failed",
    variant: "destructive",
  },
  clean: {
    label: "Clean",
    variant: "default",
    className:
      "bg-emerald-500/15 text-emerald-700 hover:bg-emerald-500/15 dark:text-emerald-300",
  },
};

function formatDuration(startedIso: string, finishedIso: string | null | undefined): string {
  if (!finishedIso) return "—";
  const ms = new Date(finishedIso).getTime() - new Date(startedIso).getTime();
  if (!Number.isFinite(ms) || ms < 0) return "—";
  if (ms < 1000) return `${ms}ms`;
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${s % 60}s`;
}

function formatTs(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString("en-AU", {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function RunsPage(): JSX.Element {
  const qc = useQueryClient();
  const [page, setPage] = useState(0);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const offset = page * PAGE_SIZE;

  // -- Import repo form state -----------------------------------------------
  const [importOwner, setImportOwner] = useState("");
  const [importName, setImportName] = useState("");
  const [importResult, setImportResult] = useState<ImportRepoResponse | null>(
    null,
  );
  const [importError, setImportError] = useState<string | null>(null);

  const importRepo = useMutation({
    mutationFn: (req: { owner: string; name: string }) =>
      api.adminImportRepo(req),
    onSuccess: (data) => {
      setImportResult(data);
      setImportError(null);
      // A fresh repo onboarded → the runs log will pick up the next
      // reconciler tick covering it. Invalidate so the table refetches.
      void qc.invalidateQueries({ queryKey: ["admin-runs"] });
    },
    onError: (err) => {
      setImportError(err instanceof Error ? err.message : String(err));
      setImportResult(null);
    },
  });

  function submitImport(e: React.FormEvent): void {
    e.preventDefault();
    setImportError(null);
    setImportResult(null);
    const owner = importOwner.trim();
    const name = importName.trim();
    if (!owner || !name) {
      setImportError("Both owner and name are required.");
      return;
    }
    importRepo.mutate({ owner, name });
  }

  function toggleExpanded(id: string): void {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  const runsQuery = useQuery({
    queryKey: ["admin-runs", PAGE_SIZE, offset],
    queryFn: () =>
      USE_MOCK
        ? Promise.resolve(paginateMockRuns(PAGE_SIZE, offset))
        : api.listRuns({ limit: PAGE_SIZE, offset }),
    refetchInterval: REFRESH_MS,
    refetchIntervalInBackground: false,
  });

  const runs = runsQuery.data ?? [];
  const hasNext = runs.length === PAGE_SIZE;
  const hasPrev = page > 0;

  return (
    <div className="flex flex-col gap-4 px-4 md:gap-6 lg:px-6">
      <PageHeading
        title="Fetch runs"
        description={
          <>
            <code className="font-mono text-xs">GET /admin/runs</code> ·
            paginated reconciler/backfill log, auto-refreshing every{" "}
            {Math.round(REFRESH_MS / 1000)}s.
          </>
        }
        trailing={
          <>
            <span
              data-testid="runs-refresh-status"
              className="text-xs text-muted-foreground"
            >
              {runsQuery.isFetching ? "Refreshing…" : "Live"}
            </span>
            <Button
              size="sm"
              variant="outline"
              onClick={() => void runsQuery.refetch()}
              disabled={runsQuery.isFetching}
              data-testid="runs-refresh"
            >
              Refresh now
            </Button>
          </>
        }
      />

      <Card data-testid="import-repo-card">
        <CardHeader>
          <CardTitle className="text-lg font-medium">Import repo</CardTitle>
        </CardHeader>
        <CardContent>
          <form
            className="grid gap-3 sm:grid-cols-[1fr_1fr_auto] sm:items-end"
            onSubmit={submitImport}
          >
            <div className="grid gap-1.5">
              <Label htmlFor="import-repo-owner">Owner</Label>
              <Input
                id="import-repo-owner"
                data-testid="import-repo-owner"
                placeholder="NubeIO"
                value={importOwner}
                onChange={(e) => setImportOwner(e.target.value)}
                disabled={importRepo.isPending}
                autoComplete="off"
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="import-repo-name">Repo name</Label>
              <Input
                id="import-repo-name"
                data-testid="import-repo-name"
                placeholder="zc-daikin"
                value={importName}
                onChange={(e) => setImportName(e.target.value)}
                disabled={importRepo.isPending}
                autoComplete="off"
              />
            </div>
            <Button
              type="submit"
              data-testid="import-repo-submit"
              disabled={importRepo.isPending}
            >
              {importRepo.isPending ? "Importing…" : "Import repo"}
            </Button>
          </form>

          {importError ? (
            <Alert
              variant="destructive"
              data-testid="import-repo-error"
              className="mt-4"
            >
              <AlertTitle>Import failed</AlertTitle>
              <AlertDescription>{importError}</AlertDescription>
            </Alert>
          ) : importResult ? (
            <Alert
              data-testid="import-repo-result"
              data-created={importResult.created}
              className="mt-4"
              aria-live="polite"
            >
              <AlertTitle>
                {importResult.created
                  ? "Repo registered."
                  : "Repo already registered."}
              </AlertTitle>
              <AlertDescription>
                <code className="font-mono text-xs">
                  org_id={importResult.org_id.slice(0, 8)} · repo_id=
                  {importResult.repo_id.slice(0, 8)}
                </code>
              </AlertDescription>
            </Alert>
          ) : null}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-medium">Recent runs</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          {runsQuery.error ? (
            <Alert variant="destructive" data-testid="runs-error">
              <AlertTitle>Failed to load runs</AlertTitle>
              <AlertDescription>{runsQuery.error.message}</AlertDescription>
            </Alert>
          ) : null}

          {runsQuery.isPending && runs.length === 0 ? (
            <p className="text-sm text-muted-foreground">Loading runs…</p>
          ) : runs.length === 0 ? (
            <p data-testid="runs-empty" className="text-sm text-muted-foreground">
              {hasPrev ? "No more runs on this page." : "No fetch runs recorded yet."}
            </p>
          ) : (
            <div className="overflow-hidden rounded-md border border-border bg-card">
              <Table data-testid="runs-table">
                <TableHeader className="bg-muted/50">
                  <TableRow>
                    <TableHead className="w-8" />
                    <TableHead>Kind</TableHead>
                    <TableHead>Started</TableHead>
                    <TableHead>Finished</TableHead>
                    <TableHead className="text-right">Duration</TableHead>
                    <TableHead className="text-right">Items</TableHead>
                    <TableHead className="text-right">Errors</TableHead>
                    <TableHead>Status</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {runs.map((r) => {
                    const status = statusOf(r);
                    const meta = STATUS_META[status];
                    const samples = r.error_sample ?? [];
                    const canExpand = samples.length > 0;
                    const isOpen = expanded.has(r.id);
                    return (
                      <Fragment key={r.id}>
                        <TableRow
                          data-run-id={r.id}
                          data-run-status={status}
                          className={canExpand ? "cursor-pointer" : undefined}
                          onClick={
                            canExpand ? () => toggleExpanded(r.id) : undefined
                          }
                        >
                          <TableCell className="w-8 px-2 text-muted-foreground">
                            {canExpand ? (
                              isOpen ? (
                                <ChevronDown
                                  className="h-4 w-4"
                                  data-testid="run-expand-open"
                                />
                              ) : (
                                <ChevronRight
                                  className="h-4 w-4"
                                  data-testid="run-expand-closed"
                                />
                              )
                            ) : null}
                          </TableCell>
                          <TableCell>
                            <code className="font-mono text-xs">{r.kind}</code>
                          </TableCell>
                          <TableCell className="text-sm">
                            {formatTs(r.started)}
                          </TableCell>
                          <TableCell className="text-sm">
                            {formatTs(r.finished ?? null)}
                          </TableCell>
                          <TableCell className="text-right text-sm tabular-nums">
                            {formatDuration(r.started, r.finished ?? null)}
                          </TableCell>
                          <TableCell className="text-right text-sm tabular-nums">
                            {r.items.toLocaleString()}
                          </TableCell>
                          <TableCell
                            className={cn(
                              "text-right text-sm tabular-nums",
                              r.errors > 0 && "text-destructive",
                            )}
                          >
                            {r.errors.toLocaleString()}
                          </TableCell>
                          <TableCell>
                            <Badge
                              variant={meta.variant}
                              data-testid="run-status-badge"
                              className={meta.className}
                            >
                              {meta.label}
                            </Badge>
                          </TableCell>
                        </TableRow>
                        {canExpand && isOpen ? (
                          <TableRow
                            data-testid="run-error-detail"
                            data-run-id={r.id}
                            className="bg-muted/30"
                          >
                            <TableCell />
                            <TableCell colSpan={7} className="py-3">
                              <div className="text-xs text-muted-foreground mb-2">
                                Showing {samples.length} of {r.errors.toLocaleString()}{" "}
                                error{r.errors === 1 ? "" : "s"} captured during this run.
                              </div>
                              <div className="overflow-hidden rounded border border-border bg-background">
                                <Table>
                                  <TableHeader>
                                    <TableRow>
                                      <TableHead className="w-40">Scope</TableHead>
                                      <TableHead className="w-32">Kind</TableHead>
                                      <TableHead>Error</TableHead>
                                    </TableRow>
                                  </TableHeader>
                                  <TableBody>
                                    {samples.map((s, i) => (
                                      <TableRow key={i}>
                                        <TableCell className="text-xs font-mono align-top">
                                          {s.repo ?? s.org ?? "—"}
                                        </TableCell>
                                        <TableCell className="text-xs font-mono align-top">
                                          {s.kind ?? "—"}
                                        </TableCell>
                                        <TableCell className="text-xs font-mono whitespace-pre-wrap break-words align-top text-destructive">
                                          {s.error}
                                        </TableCell>
                                      </TableRow>
                                    ))}
                                  </TableBody>
                                </Table>
                              </div>
                            </TableCell>
                          </TableRow>
                        ) : null}
                      </Fragment>
                    );
                  })}
                </TableBody>
              </Table>
            </div>
          )}

          <div className="flex items-center justify-between pt-1">
            <span className="text-xs text-muted-foreground">
              Page {page + 1} · showing rows {offset + 1}–{offset + runs.length}
            </span>
            <div className="flex gap-2">
              <Button
                size="sm"
                variant="outline"
                disabled={!hasPrev}
                onClick={() => setPage((p) => Math.max(0, p - 1))}
                data-testid="runs-prev"
              >
                Previous
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={!hasNext}
                onClick={() => setPage((p) => p + 1)}
                data-testid="runs-next"
              >
                Next
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

export const __test__ = { statusOf, formatDuration };
