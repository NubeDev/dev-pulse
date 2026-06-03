/**
 * `#/runs/{id}` — production run detail (§7.4, P2).
 *
 * Header with the four counters as stat cards + a derived yield %
 * (pass / built); a status control (planned → in_progress →
 * completed / cancelled via `patchRun`); the run's units table; an
 * "Add units" dialog (allocate N serials, shows the reserved range);
 * and a Run EOL sign-off card.
 *
 * PATCH carries `expected_version`; a 409 surfaces a "changed
 * underneath you" message and triggers a refetch, mirroring P1.
 */

import { useEffect, useState } from "react";
import { useAuth } from "@nube/starter-ui-core/auth";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";

import { isDpRestError } from "../../api/error.js";
import type {
  RunDto,
  RunStatus,
  UnitAllocationDto,
} from "../../api/schemas/products.js";
import { Markdown } from "../../components/markdown.jsx";
import { PageHeading } from "../../components/page-heading.jsx";
import {
  navigate,
  productDetailTabRoute,
  unitDetailRoute,
} from "../../routes.js";
import {
  useAllocateUnits,
  usePatchRun,
  useRun,
  useRunEolSummary,
  useRunUnits,
  useUpsertRunEolSummary,
} from "../use-manufacturing-data.js";

import {
  RUN_STATUSES,
  RUN_STATUS_LABEL,
  RUN_STATUS_VARIANT,
  UNIT_STATUS_LABEL,
  UNIT_STATUS_VARIANT,
} from "./run-shared.js";

export function RunDetail({ runId }: { runId: string }): JSX.Element {
  const run = useRun(runId);

  if (run.isPending) {
    return (
      <div className="px-4 lg:px-6">
        <div
          className="flex items-center gap-2 py-4 text-sm text-muted-foreground"
          data-testid="run-detail-loading"
        >
          <Spinner /> Loading run…
        </div>
      </div>
    );
  }

  if (run.isError) {
    return (
      <div className="px-4 lg:px-6">
        <Alert variant="destructive" data-testid="run-detail-error">
          <AlertTitle>Couldn't load run</AlertTitle>
          <AlertDescription>{run.error.message}</AlertDescription>
        </Alert>
      </div>
    );
  }

  if (!run.data) {
    return (
      <div className="px-4 lg:px-6">
        <Alert data-testid="run-detail-missing">
          <AlertTitle>Run not found</AlertTitle>
          <AlertDescription>
            This run either doesn't exist or you don't have access to it.{" "}
            <a className="underline" href="#/products">
              Back to products
            </a>
            .
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  return <RunDetailBody run={run.data} />;
}

function RunDetailBody({ run }: { run: RunDto }): JSX.Element {
  const patch = usePatchRun(run.id, run.product_id);
  const [addOpen, setAddOpen] = useState(false);

  const yieldPct =
    run.qty_built > 0
      ? Math.round((run.qty_passed / run.qty_built) * 100)
      : null;

  const conflict = isDpRestError(patch.error) && patch.error.status === 409;

  const onStatus = (next: RunStatus): void => {
    if (next === run.status) return;
    patch.mutate({
      expected_version: run.version,
      manufacturer_id: run.manufacturer_id ?? null,
      run_code: run.run_code,
      status: next,
      qty_planned: run.qty_planned,
      started_at: run.started_at ?? null,
      completed_at: run.completed_at ?? null,
      notes: run.notes ?? null,
    });
  };

  return (
    <div className="flex flex-col gap-4 px-4 lg:px-6" data-testid="run-detail">
      <PageHeading
        title={
          <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="font-mono" data-testid="run-detail-code">
              {run.run_code}
            </span>
            <Badge variant={RUN_STATUS_VARIANT[run.status]}>
              {RUN_STATUS_LABEL[run.status]}
            </Badge>
          </span>
        }
        description={
          <a
            className="text-sm text-muted-foreground underline"
            href={productDetailTabRoute(run.product_id, "overview")}
          >
            Back to product
          </a>
        }
        trailing={
          <div className="flex items-center gap-2">
            <Select
              value={run.status}
              onValueChange={(v) => onStatus(v as RunStatus)}
            >
              <SelectTrigger
                className="h-9 w-44"
                data-testid="run-status-control"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {RUN_STATUSES.map((s) => (
                  <SelectItem key={s} value={s}>
                    {RUN_STATUS_LABEL[s]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              onClick={() => setAddOpen(true)}
              size="sm"
              data-testid="run-add-units"
            >
              Add units
            </Button>
          </div>
        }
      />

      {conflict && (
        <Alert variant="destructive" data-testid="run-detail-conflict">
          <AlertTitle>This run changed underneath you</AlertTitle>
          <AlertDescription>
            Someone else updated this run. The latest values have been
            reloaded — re-apply your change.
          </AlertDescription>
        </Alert>
      )}
      {patch.isError && !conflict && (
        <Alert variant="destructive" data-testid="run-detail-patch-error">
          <AlertTitle>Couldn't update run</AlertTitle>
          <AlertDescription>{patch.error.message}</AlertDescription>
        </Alert>
      )}

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-5">
        <StatCard label="Planned" value={run.qty_planned} />
        <StatCard label="Built" value={run.qty_built} />
        <StatCard
          label="Pass"
          value={run.qty_passed}
          className="text-emerald-600"
        />
        <StatCard
          label="Fail"
          value={run.qty_failed}
          className="text-destructive"
        />
        <StatCard
          label="Yield"
          value={yieldPct === null ? "—" : `${yieldPct}%`}
        />
      </div>

      <RunUnitsCard runId={run.id} />

      <RunEolSignOffCard run={run} />

      <AddUnitsDialog open={addOpen} onOpenChange={setAddOpen} run={run} />
    </div>
  );
}

function StatCard({
  label,
  value,
  className,
}: {
  label: string;
  value: number | string;
  className?: string;
}): JSX.Element {
  return (
    <Card className="gap-1 py-3" data-testid={`run-stat-${label.toLowerCase()}`}>
      <CardHeader className="px-4">
        <CardTitle className="text-xs font-medium text-muted-foreground">
          {label}
        </CardTitle>
      </CardHeader>
      <CardContent className="px-4">
        <span className={`text-2xl font-semibold tabular-nums ${className ?? ""}`}>
          {value}
        </span>
      </CardContent>
    </Card>
  );
}

function RunUnitsCard({ runId }: { runId: string }): JSX.Element {
  const units = useRunUnits(runId);
  const rows = units.data ?? [];

  return (
    <Card className="gap-2 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-sm">
          Units{" "}
          <span className="text-muted-foreground">({rows.length})</span>
        </CardTitle>
      </CardHeader>
      <CardContent className="px-4">
        {units.isError ? (
          <Alert variant="destructive" data-testid="run-units-error">
            <AlertDescription>{units.error.message}</AlertDescription>
          </Alert>
        ) : units.isPending ? (
          <div className="flex flex-col gap-2">
            {Array.from({ length: 3 }).map((_, i) => (
              <Skeleton key={i} className="h-10 rounded-md" />
            ))}
          </div>
        ) : rows.length === 0 ? (
          <p
            className="py-6 text-center text-sm text-muted-foreground"
            data-testid="run-units-empty"
          >
            No units allocated yet. Use "Add units" to reserve serials.
          </p>
        ) : (
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Serial</TableHead>
                  <TableHead>Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((u) => (
                  <TableRow
                    key={u.id}
                    className="cursor-pointer"
                    onClick={() => navigate(unitDetailRoute(u.id))}
                    data-testid="run-unit-row"
                  >
                    <TableCell className="font-mono">
                      {u.serial_number}
                    </TableCell>
                    <TableCell>
                      <Badge variant={UNIT_STATUS_VARIANT[u.status]}>
                        {UNIT_STATUS_LABEL[u.status]}
                      </Badge>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function AddUnitsDialog({
  open,
  onOpenChange,
  run,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  run: RunDto;
}): JSX.Element {
  const allocate = useAllocateUnits(run.id);
  const [count, setCount] = useState("1");
  const [result, setResult] = useState<UnitAllocationDto | null>(null);

  useEffect(() => {
    if (!open) return;
    setCount("1");
    setResult(null);
    allocate.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const n = Number(count);
  const valid = Number.isInteger(n) && n >= 1 && n <= 1000;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!valid) return;
    allocate.mutate(
      { count: n },
      { onSuccess: (data) => setResult(data) },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md" data-testid="add-units-dialog">
        <DialogHeader>
          <DialogTitle>Add units</DialogTitle>
          <DialogDescription>
            Reserve a block of serial numbers against {run.run_code}.
          </DialogDescription>
        </DialogHeader>

        {result ? (
          <div
            className="flex flex-col gap-3"
            data-testid="add-units-result"
          >
            <Alert>
              <AlertTitle>
                Reserved {result.count} serial
                {result.count === 1 ? "" : "s"}
              </AlertTitle>
              <AlertDescription>
                Sequence {result.first_seq} –{" "}
                {result.first_seq + result.count - 1}.
              </AlertDescription>
            </Alert>
            <div className="max-h-56 overflow-y-auto rounded-md border">
              <ul className="divide-y text-sm">
                {result.units.map((u) => (
                  <li
                    key={u.id}
                    className="flex items-center justify-between px-3 py-2"
                  >
                    <button
                      type="button"
                      className="font-mono hover:underline"
                      onClick={() => navigate(unitDetailRoute(u.id))}
                    >
                      {u.serial_number}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
            <DialogFooter>
              <Button onClick={() => onOpenChange(false)}>Done</Button>
            </DialogFooter>
          </div>
        ) : (
          <form className="flex flex-col gap-4" onSubmit={onSubmit}>
            <div className="flex flex-col gap-2">
              <Label htmlFor="add-units-count">Quantity (1–1000)</Label>
              <Input
                id="add-units-count"
                data-testid="add-units-count"
                type="number"
                min={1}
                max={1000}
                value={count}
                onChange={(e) => setCount(e.target.value)}
                autoFocus
              />
              {!valid && (
                <p className="text-xs text-destructive">
                  Enter a whole number between 1 and 1000.
                </p>
              )}
            </div>

            {allocate.isError && (
              <Alert variant="destructive" data-testid="add-units-error">
                <AlertTitle>Allocation failed</AlertTitle>
                <AlertDescription>{allocate.error.message}</AlertDescription>
              </Alert>
            )}

            <DialogFooter>
              <Button
                type="button"
                variant="ghost"
                onClick={() => onOpenChange(false)}
                disabled={allocate.isPending}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                data-testid="add-units-submit"
                disabled={!valid || allocate.isPending}
              >
                {allocate.isPending ? "Allocating…" : "Allocate"}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}

function RunEolSignOffCard({ run }: { run: RunDto }): JSX.Element {
  const summary = useRunEolSummary(run.id);
  const upsert = useUpsertRunEolSummary(run.id);
  const auth = useAuth();
  const [notes, setNotes] = useState("");

  // Seed the notes editor from the loaded summary once.
  useEffect(() => {
    setNotes(summary.data?.notes_md ?? "");
  }, [summary.data?.notes_md]);

  const data = summary.data;
  const signed = !!data?.signed_at;

  const onSaveNotes = (): void => {
    upsert.mutate({ notes_md: notes.trim() || null });
  };
  const onSignOff = (): void => {
    upsert.mutate({ notes_md: notes.trim() || null, sign_off: true });
  };

  return (
    <Card className="gap-2 py-4" data-testid="run-eol-signoff">
      <CardHeader className="px-4">
        <CardTitle className="text-sm">EOL sign-off</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 px-4">
        {summary.isError ? (
          <Alert variant="destructive">
            <AlertDescription>{summary.error.message}</AlertDescription>
          </Alert>
        ) : summary.isPending ? (
          <Skeleton className="h-20 rounded-md" />
        ) : (
          <>
            <div className="grid grid-cols-3 gap-2 text-sm">
              <Snapshot label="Built" value={data?.built_count ?? run.qty_built} />
              <Snapshot
                label="Pass"
                value={data?.pass_count ?? run.qty_passed}
              />
              <Snapshot
                label="Fail"
                value={data?.fail_count ?? run.qty_failed}
              />
            </div>

            {signed ? (
              <Alert data-testid="run-eol-signed">
                <AlertTitle>Signed off</AlertTitle>
                <AlertDescription>
                  {data?.signed_by ? `By ${data.signed_by} · ` : ""}
                  {data?.signed_at
                    ? new Date(data.signed_at).toLocaleString()
                    : ""}
                </AlertDescription>
              </Alert>
            ) : null}

            <div className="flex flex-col gap-2">
              <Label htmlFor="run-eol-notes">Notes (markdown)</Label>
              <Textarea
                id="run-eol-notes"
                data-testid="run-eol-notes"
                rows={4}
                value={notes}
                onChange={(e) => setNotes(e.target.value)}
                placeholder="Sign-off notes, deviations, sampling plan…"
              />
              {notes.trim() ? (
                <div className="rounded-md border bg-background p-3">
                  <Markdown>{notes}</Markdown>
                </div>
              ) : null}
            </div>

            {upsert.isError && (
              <Alert variant="destructive" data-testid="run-eol-error">
                <AlertDescription>{upsert.error.message}</AlertDescription>
              </Alert>
            )}

            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={onSaveNotes}
                disabled={upsert.isPending}
                data-testid="run-eol-save-notes"
              >
                Save notes
              </Button>
              {!signed && (
                <Button
                  size="sm"
                  onClick={onSignOff}
                  disabled={upsert.isPending}
                  data-testid="run-eol-signoff-button"
                  title={
                    auth.user ? `Sign off as ${auth.user.email}` : "Sign off"
                  }
                >
                  {upsert.isPending ? "Saving…" : "Sign off"}
                </Button>
              )}
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}

function Snapshot({
  label,
  value,
}: {
  label: string;
  value: number;
}): JSX.Element {
  return (
    <div className="rounded-md bg-muted/40 px-3 py-2">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="text-lg font-semibold tabular-nums">{value}</div>
    </div>
  );
}
