import { useEffect, useRef, useState } from "react";
import { DownloadIcon, FileIcon, TrashIcon, UploadCloudIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";

import type { ExecSummaryDto } from "../../../api/client.js";
import {
  useDeleteExecSummaryDocument,
  usePatchExecSummaryDocument,
  useUploadExecSummaryDocument,
} from "../hooks/use-exec-summary.js";

const DOC_TYPES = [
  { value: "brief", label: "Brief" },
  { value: "bom", label: "BOM" },
  { value: "datasheet", label: "Datasheet" },
  { value: "sketch", label: "Sketch" },
  { value: "spec", label: "Specification" },
  { value: "other", label: "Other" },
];

export function DocumentsSection({
  projectId,
  data,
}: {
  projectId: string;
  data: ExecSummaryDto;
}): JSX.Element {
  const upload = useUploadExecSummaryDocument(projectId);
  const remove = useDeleteExecSummaryDocument(projectId);
  const patchDoc = usePatchExecSummaryDocument(projectId);
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">
          Documents{" "}
          <span className="text-muted-foreground">({data.documents.length})</span>
        </h3>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => inputRef.current?.click()}
        >
          <UploadCloudIcon className="mr-1.5 h-4 w-4" /> Add document
        </Button>
        <input
          ref={inputRef}
          type="file"
          className="hidden"
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) setPendingFile(file);
            e.target.value = "";
          }}
        />
      </div>

      {upload.isError && (
        <Alert variant="destructive">
          <AlertTitle>Upload failed</AlertTitle>
          <AlertDescription>{upload.error.message}</AlertDescription>
        </Alert>
      )}

      {data.documents.length === 0 ? (
        <div className="rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground">
          No documents attached yet. Add briefs, BOMs, datasheets, or anything
          else that backs this project.
        </div>
      ) : (
        <ul className="flex flex-col divide-y rounded-md border">
          {data.documents.map((doc) => (
            <li
              key={doc.id}
              className="flex items-start gap-3 px-3 py-3 text-sm"
            >
              <FileIcon className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
              <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                <div className="flex flex-wrap items-center gap-2">
                  <a
                    href={doc.url}
                    target="_blank"
                    rel="noreferrer"
                    className="truncate font-medium hover:underline"
                  >
                    {doc.title}
                  </a>
                  {doc.doc_type && (
                    <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] uppercase tracking-wide text-muted-foreground">
                      {doc.doc_type}
                    </span>
                  )}
                  <span className="text-xs text-muted-foreground">
                    {doc.filename}
                  </span>
                </div>
                <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
                  <Input
                    defaultValue={doc.notes ?? ""}
                    placeholder="Notes"
                    onBlur={(e) => {
                      const next = e.target.value.trim() || null;
                      if (next === (doc.notes ?? null)) return;
                      patchDoc.mutate({
                        documentId: doc.id,
                        body: { notes: next },
                      });
                    }}
                  />
                  <Input
                    defaultValue={doc.required_action ?? ""}
                    placeholder="Required action"
                    onBlur={(e) => {
                      const next = e.target.value.trim() || null;
                      if (next === (doc.required_action ?? null)) return;
                      patchDoc.mutate({
                        documentId: doc.id,
                        body: { required_action: next },
                      });
                    }}
                  />
                </div>
              </div>
              <a
                href={doc.url}
                target="_blank"
                rel="noreferrer"
                className="text-muted-foreground hover:text-foreground"
                title="Download"
              >
                <DownloadIcon className="h-4 w-4" />
              </a>
              <button
                type="button"
                onClick={() => remove.mutate(doc.id)}
                disabled={remove.isPending}
                className={cn(
                  "text-muted-foreground hover:text-destructive",
                  remove.isPending && "opacity-50",
                )}
                title="Remove"
              >
                <TrashIcon className="h-4 w-4" />
              </button>
            </li>
          ))}
        </ul>
      )}

      <NewDocumentDialog
        projectId={projectId}
        file={pendingFile}
        onOpenChange={(open) => {
          if (!open) setPendingFile(null);
        }}
      />
    </div>
  );
}

function NewDocumentDialog({
  projectId: _projectId,
  file,
  onOpenChange,
}: {
  projectId: string;
  file: File | null;
  onOpenChange: (open: boolean) => void;
}): JSX.Element {
  const upload = useUploadExecSummaryDocument(_projectId);
  const [title, setTitle] = useState("");
  const [docType, setDocType] = useState<string>("brief");
  const [notes, setNotes] = useState("");
  const [requiredAction, setRequiredAction] = useState("");

  const open = file !== null;
  // Seed title from the filename whenever a new file is queued.
  useEffect(() => {
    if (file) setTitle(file.name.replace(/\.[^.]+$/, ""));
  }, [file]);

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!file || !title.trim()) return;
    upload.mutate(
      {
        file,
        title: title.trim(),
        doc_type: docType,
        notes: notes.trim() || undefined,
        required_action: requiredAction.trim() || undefined,
      },
      {
        onSuccess: () => {
          setTitle("");
          setDocType("brief");
          setNotes("");
          setRequiredAction("");
          onOpenChange(false);
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add document</DialogTitle>
          <DialogDescription>
            {file?.name ?? ""} · {file ? Math.round(file.size / 1024) : 0} KB
          </DialogDescription>
        </DialogHeader>
        <form className="flex flex-col gap-3" onSubmit={onSubmit}>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="doc-title">Title</Label>
            <Input
              id="doc-title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              autoFocus
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="doc-type">Type</Label>
            <Select value={docType} onValueChange={setDocType}>
              <SelectTrigger id="doc-type">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {DOC_TYPES.map((t) => (
                  <SelectItem key={t.value} value={t.value}>
                    {t.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="doc-notes">Notes</Label>
            <Textarea
              id="doc-notes"
              rows={2}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="doc-action">Required action</Label>
            <Input
              id="doc-action"
              value={requiredAction}
              onChange={(e) => setRequiredAction(e.target.value)}
              placeholder="e.g. Review by 2026-06-01"
            />
          </div>
          {upload.isError && (
            <Alert variant="destructive">
              <AlertDescription>{upload.error.message}</AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={upload.isPending}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={upload.isPending || !title.trim()}
            >
              {upload.isPending ? "Uploading…" : "Add"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
