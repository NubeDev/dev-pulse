/**
 * "Save version" dialog for the Executive Summary.
 *
 * The summary autosaves continuously as a draft; this dialog is the
 * deliberate act of cutting a *named version* into the append-only
 * change log. The user only decides whether it's a **minor** or
 * **major** update — the version label (`v1`, `v1.1`, `v2`, …) is
 * derived for them (see `computeNextVersion`).
 */

import { useMemo, useState } from "react";
import { Loader2Icon, SaveIcon } from "lucide-react";

import { useAuth } from "@nube/starter-ui-core/auth";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";

import type { ExecSummaryDto } from "../../api/client.js";
import { formatAu } from "../view-wizard/date-display.js";
import { useAddExecSummaryChangelog } from "./hooks/use-exec-summary.js";
import { computeNextVersion, type VersionBump } from "./version.js";

export function SaveVersionDialog({
  projectId,
  data,
  open,
  onOpenChange,
}: {
  projectId: string;
  data: ExecSummaryDto;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): JSX.Element {
  const auth = useAuth();
  const add = useAddExecSummaryChangelog(projectId);

  const [bump, setBump] = useState<VersionBump>("minor");
  const [summary, setSummary] = useState("");

  const today = new Date().toISOString().slice(0, 10);
  const changedBy = useMemo(() => {
    const email = auth.user?.email;
    if (!email) return "unknown";
    return email.split("@")[0] || email;
  }, [auth.user?.email]);

  const nextVersion = useMemo(
    () => computeNextVersion(data.changelog, bump),
    [data.changelog, bump],
  );

  const canSave = summary.trim().length > 0 && !add.isPending;

  const handleSave = (): void => {
    if (!canSave) return;
    add.mutate(
      {
        version: nextVersion,
        changed_at: today,
        changed_by: changedBy,
        summary: summary.trim(),
      },
      {
        onSuccess: () => {
          setSummary("");
          setBump("minor");
          onOpenChange(false);
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Save a new version</DialogTitle>
          <DialogDescription>
            This records the current summary as version{" "}
            <span className="font-semibold text-foreground">
              {nextVersion}
            </span>{" "}
            in the change log. Pick whether it's a minor or major update.
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label className="text-xs">Update type</Label>
            <div className="grid grid-cols-2 gap-2">
              <BumpOption
                label="Minor"
                hint="Small change · v1 → v1.1"
                selected={bump === "minor"}
                onClick={() => setBump("minor")}
              />
              <BumpOption
                label="Major"
                hint="Big change · v1 → v2"
                selected={bump === "major"}
                onClick={() => setBump("major")}
              />
            </div>
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="save-version-summary" className="text-xs">
              What changed?
            </Label>
            <Textarea
              id="save-version-summary"
              rows={4}
              value={summary}
              onChange={(e) => setSummary(e.target.value)}
              placeholder="Summarise what changed in this revision…"
              autoFocus
            />
          </div>

          <p className="text-xs text-muted-foreground">
            Saving as{" "}
            <span className="font-mono font-medium text-foreground">
              {nextVersion}
            </span>{" "}
            · {formatAu(today)} · {changedBy}
          </p>

          {add.isError && (
            <Alert variant="destructive">
              <AlertDescription>{add.error.message}</AlertDescription>
            </Alert>
          )}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={() => onOpenChange(false)}
            disabled={add.isPending}
          >
            Cancel
          </Button>
          <Button type="button" onClick={handleSave} disabled={!canSave}>
            {add.isPending ? (
              <>
                <Loader2Icon className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                Saving…
              </>
            ) : (
              <>
                <SaveIcon className="mr-1.5 h-3.5 w-3.5" />
                Save {nextVersion}
              </>
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function BumpOption({
  label,
  hint,
  selected,
  onClick,
}: {
  label: string;
  hint: string;
  selected: boolean;
  onClick: () => void;
}): JSX.Element {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={selected}
      className={cn(
        "flex flex-col items-start gap-0.5 rounded-md border px-3 py-2 text-left transition-colors",
        selected
          ? "border-primary bg-primary/5 ring-1 ring-primary"
          : "border-border hover:bg-muted/50",
      )}
    >
      <span className="text-sm font-medium">{label}</span>
      <span className="text-[11px] text-muted-foreground">{hint}</span>
    </button>
  );
}
