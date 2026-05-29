import { useState } from "react";
import { PlusIcon, TrashIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { DateInput } from "@/components/ui/date-input";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

import type { ExecSummaryDto } from "../../../api/client.js";
import {
  useAddExecSummaryChangelog,
  useDeleteExecSummaryChangelog,
} from "../hooks/use-exec-summary.js";

export function ChangelogSection({
  projectId,
  data,
}: {
  projectId: string;
  data: ExecSummaryDto;
}): JSX.Element {
  const add = useAddExecSummaryChangelog(projectId);
  const remove = useDeleteExecSummaryChangelog(projectId);
  const [deleteId, setDeleteId] = useState<string | null>(null);

  const [version, setVersion] = useState("");
  const [changedAt, setChangedAt] = useState(() =>
    new Date().toISOString().slice(0, 10),
  );
  const [changedBy, setChangedBy] = useState("");
  const [summary, setSummary] = useState("");

  const canSubmit =
    version.trim().length > 0 &&
    changedAt.length === 10 &&
    changedBy.trim().length > 0 &&
    summary.trim().length > 0;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!canSubmit) return;
    add.mutate(
      {
        version: version.trim(),
        changed_at: changedAt,
        changed_by: changedBy.trim(),
        summary: summary.trim(),
      },
      {
        onSuccess: () => {
          setVersion("");
          setChangedBy("");
          setSummary("");
        },
      },
    );
  };

  return (
    <div className="flex flex-col gap-6" data-validation-key="changelog.any">
      <form
        onSubmit={onSubmit}
        className="grid grid-cols-1 gap-3 rounded-md border bg-muted/20 p-3 md:grid-cols-[120px_140px_160px_1fr_auto]"
      >
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="cl-version" className="text-xs">
            Version
          </Label>
          <Input
            id="cl-version"
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            placeholder="0.1.0"
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="cl-date" className="text-xs">
            Date
          </Label>
          <DateInput
            id="cl-date"
            value={changedAt}
            onChange={(e) => setChangedAt(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="cl-author" className="text-xs">
            Author
          </Label>
          <Input
            id="cl-author"
            value={changedBy}
            onChange={(e) => setChangedBy(e.target.value)}
            placeholder="@you"
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="cl-summary" className="text-xs">
            Summary
          </Label>
          <Textarea
            id="cl-summary"
            rows={2}
            value={summary}
            onChange={(e) => setSummary(e.target.value)}
            placeholder="What changed in this revision?"
          />
        </div>
        <div className="flex items-end">
          <Button
            type="submit"
            size="sm"
            disabled={!canSubmit || add.isPending}
          >
            <PlusIcon className="mr-1 h-3.5 w-3.5" />
            {add.isPending ? "Adding…" : "Add entry"}
          </Button>
        </div>
        {add.isError && (
          <Alert variant="destructive" className="md:col-span-5">
            <AlertDescription>{add.error.message}</AlertDescription>
          </Alert>
        )}
      </form>

      {data.changelog.length === 0 ? (
        <div className="rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground">
          No entries yet. The change log is append-only — each revision of
          the summary should land here.
        </div>
      ) : (
        <ul className="flex flex-col divide-y rounded-md border">
          {data.changelog.map((entry) => (
            <li
              key={entry.id}
              className="grid grid-cols-1 gap-2 px-3 py-3 text-sm md:grid-cols-[100px_120px_160px_1fr_auto] md:items-start"
            >
              <span className="font-mono text-xs text-muted-foreground">
                {entry.version}
              </span>
              <span className="tabular-nums text-xs text-muted-foreground">
                {entry.changed_at}
              </span>
              <span className="truncate text-xs text-muted-foreground">
                {entry.changed_by}
              </span>
              <span className="whitespace-pre-wrap">{entry.summary}</span>
              <button
                type="button"
                className="self-start text-muted-foreground hover:text-destructive"
                title="Delete entry"
                onClick={() => setDeleteId(entry.id)}
              >
                <TrashIcon className="h-3.5 w-3.5" />
              </button>
            </li>
          ))}
        </ul>
      )}

      <AlertDialog
        open={deleteId !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteId(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete changelog entry?</AlertDialogTitle>
            <AlertDialogDescription>
              The change log is append-only by design. Deleting an entry
              writes an audit row and cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={remove.isPending}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={remove.isPending}
              onClick={() => {
                if (!deleteId) return;
                remove.mutate(deleteId, {
                  onSuccess: () => setDeleteId(null),
                });
              }}
            >
              {remove.isPending ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
