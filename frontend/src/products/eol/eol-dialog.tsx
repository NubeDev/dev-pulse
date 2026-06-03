/**
 * EOL record dialog (§7.4, P2).
 *
 * Records an end-of-line test report against a unit: pass / fail
 * toggle, station + firmware text, a simple key/value measurements
 * editor (rows of key+value → a `Record<string, unknown>`), notes,
 * and tester. On submit calls `api.recordEol` via `useRecordEol`.
 *
 * Raw-log file upload is NOT supported by the P2 endpoint, so it is
 * intentionally omitted.
 */

import { useEffect, useState } from "react";
import { PlusIcon, XIcon } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
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
import { Textarea } from "@/components/ui/textarea";

import type { EolResult } from "../../api/schemas/products.js";
import { useRecordEol } from "../use-manufacturing-data.js";

interface MeasurementRow {
  key: string;
  value: string;
}

export function EolDialog({
  open,
  onOpenChange,
  unitId,
  runId,
  defaultTestedBy,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  unitId: string;
  runId?: string | null;
  /** Pre-fill the tester field (e.g. the logged-in user's email). */
  defaultTestedBy?: string | null;
}): JSX.Element {
  const record = useRecordEol(unitId, runId);

  const [result, setResult] = useState<EolResult>("pass");
  const [station, setStation] = useState("");
  const [firmware, setFirmware] = useState("");
  const [notes, setNotes] = useState("");
  const [testedBy, setTestedBy] = useState(defaultTestedBy ?? "");
  const [rows, setRows] = useState<MeasurementRow[]>([{ key: "", value: "" }]);

  useEffect(() => {
    if (!open) return;
    setResult("pass");
    setStation("");
    setFirmware("");
    setNotes("");
    setTestedBy(defaultTestedBy ?? "");
    setRows([{ key: "", value: "" }]);
    record.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, defaultTestedBy]);

  const setRow = (i: number, patch: Partial<MeasurementRow>): void => {
    setRows((cur) => cur.map((r, idx) => (idx === i ? { ...r, ...patch } : r)));
  };
  const addRow = (): void =>
    setRows((cur) => [...cur, { key: "", value: "" }]);
  const removeRow = (i: number): void =>
    setRows((cur) => cur.filter((_, idx) => idx !== i));

  /** Build the measurements record — keep keys with a non-empty name,
   *  coerce numeric-looking values to numbers so downstream charts
   *  treat them as quantities. */
  const buildMeasurements = (): Record<string, unknown> => {
    const out: Record<string, unknown> = {};
    for (const { key, value } of rows) {
      const k = key.trim();
      if (k.length === 0) continue;
      const v = value.trim();
      if (v.length === 0) {
        out[k] = null;
        continue;
      }
      const n = Number(v);
      out[k] = v !== "" && !Number.isNaN(n) ? n : v;
    }
    return out;
  };

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    const measurements = buildMeasurements();
    record.mutate(
      {
        result,
        station: station.trim() || null,
        firmware: firmware.trim() || null,
        measurements:
          Object.keys(measurements).length > 0 ? measurements : undefined,
        notes: notes.trim() || null,
        tested_by: testedBy.trim() || null,
      },
      { onSuccess: () => onOpenChange(false) },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg" data-testid="eol-dialog">
        <DialogHeader>
          <DialogTitle>Record EOL test</DialogTitle>
          <DialogDescription>
            Log an end-of-line test result for this unit. A failing
            result is kept in the unit's timeline alongside passes.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
          <div className="flex flex-col gap-2">
            <Label>Result</Label>
            <div className="flex gap-2">
              <Button
                type="button"
                variant={result === "pass" ? "default" : "outline"}
                size="sm"
                onClick={() => setResult("pass")}
                data-testid="eol-result-pass"
              >
                Pass
              </Button>
              <Button
                type="button"
                variant={result === "fail" ? "destructive" : "outline"}
                size="sm"
                onClick={() => setResult("fail")}
                data-testid="eol-result-fail"
              >
                Fail
              </Button>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="flex flex-col gap-2">
              <Label htmlFor="eol-station">Station</Label>
              <Input
                id="eol-station"
                data-testid="eol-station"
                value={station}
                onChange={(e) => setStation(e.target.value)}
                placeholder="EOL-1"
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="eol-firmware">Firmware</Label>
              <Input
                id="eol-firmware"
                data-testid="eol-firmware"
                value={firmware}
                onChange={(e) => setFirmware(e.target.value)}
                placeholder="v1.2.3"
              />
            </div>
          </div>

          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <Label>Measurements</Label>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={addRow}
                data-testid="eol-add-measurement"
              >
                <PlusIcon className="mr-1 h-4 w-4" /> Add row
              </Button>
            </div>
            <div className="flex flex-col gap-2">
              {rows.map((row, i) => (
                <div key={i} className="flex items-center gap-2">
                  <Input
                    value={row.key}
                    onChange={(e) => setRow(i, { key: e.target.value })}
                    placeholder="key (e.g. vbat_mv)"
                    data-testid="eol-measurement-key"
                  />
                  <Input
                    value={row.value}
                    onChange={(e) => setRow(i, { value: e.target.value })}
                    placeholder="value"
                    data-testid="eol-measurement-value"
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="shrink-0"
                    onClick={() => removeRow(i)}
                    disabled={rows.length === 1}
                    title="Remove row"
                  >
                    <XIcon className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
            <p className="text-[11px] text-muted-foreground">
              Numeric values are stored as numbers; everything else as
              text.
            </p>
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="eol-notes">Notes</Label>
            <Textarea
              id="eol-notes"
              data-testid="eol-notes"
              rows={3}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              placeholder="Observations, deviations…"
            />
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="eol-tested-by">Tester</Label>
            <Input
              id="eol-tested-by"
              data-testid="eol-tested-by"
              value={testedBy}
              onChange={(e) => setTestedBy(e.target.value)}
              placeholder="name / email"
            />
          </div>

          {record.isError && (
            <Alert variant="destructive" data-testid="eol-error">
              <AlertTitle>Couldn't record EOL</AlertTitle>
              <AlertDescription>{record.error.message}</AlertDescription>
            </Alert>
          )}

          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={record.isPending}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              data-testid="eol-submit"
              disabled={record.isPending}
            >
              {record.isPending ? "Recording…" : "Record test"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
