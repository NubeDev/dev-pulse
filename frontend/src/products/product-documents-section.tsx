/**
 * Product documents tab — mirrors the exec-summary
 * `DocumentsSection` against the product document API
 * (`api.listProductDocuments` / `uploadProductDocument` /
 * `deleteProductDocument`). The product document DTO is leaner than
 * the exec-summary one (no `required_action`, title is patched via
 * re-upload only), so the rows here are read-only metadata + a
 * download link + a delete control.
 */

import { useEffect, useRef, useState } from "react";
import {
  DownloadIcon,
  FileIcon,
  TrashIcon,
  UploadCloudIcon,
} from "lucide-react";

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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";

import {
  useDeleteProductDocument,
  useProductDocuments,
  useUploadProductDocument,
} from "./use-products-data.js";

const DOC_TYPES = [
  { value: "manual", label: "Manual" },
  { value: "datasheet", label: "Datasheet" },
  { value: "bom", label: "BOM" },
  { value: "drawing", label: "Drawing" },
  { value: "cert", label: "Certificate" },
  { value: "other", label: "Other" },
];

export function ProductDocumentsSection({
  productId,
}: {
  productId: string;
}): JSX.Element {
  const docs = useProductDocuments(productId);
  const remove = useDeleteProductDocument(productId);
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const rows = docs.data ?? [];

  return (
    <div className="flex flex-col gap-4" data-testid="product-documents">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">
          Documents{" "}
          <span className="text-muted-foreground">({rows.length})</span>
        </h3>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => inputRef.current?.click()}
          data-testid="product-document-add"
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

      {docs.isError ? (
        <Alert variant="destructive" data-testid="product-documents-error">
          <AlertTitle>Couldn't load documents</AlertTitle>
          <AlertDescription>{docs.error.message}</AlertDescription>
        </Alert>
      ) : docs.isPending ? (
        <div className="flex flex-col gap-2" data-testid="product-documents-loading">
          {Array.from({ length: 3 }).map((_, i) => (
            <Skeleton key={i} className="h-12 rounded-md" />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <div
          className="rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground"
          data-testid="product-documents-empty"
        >
          No documents attached yet. Add datasheets, BOMs, drawings, or
          certificates that back this product.
        </div>
      ) : (
        <ul className="flex flex-col divide-y rounded-md border">
          {rows.map((doc) => (
            <li
              key={doc.id}
              className="flex items-start gap-3 px-3 py-3 text-sm"
              data-testid="product-document-row"
            >
              <FileIcon className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate font-medium" title={doc.title}>
                  {doc.title}
                </span>
                <span className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                  {doc.doc_type ? (
                    <span className="rounded bg-muted px-1.5 py-0.5 uppercase">
                      {doc.doc_type}
                    </span>
                  ) : null}
                  {doc.notes ? <span className="truncate">{doc.notes}</span> : null}
                </span>
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
                data-testid="product-document-delete"
              >
                <TrashIcon className="h-4 w-4" />
              </button>
            </li>
          ))}
        </ul>
      )}

      <NewProductDocumentDialog
        productId={productId}
        file={pendingFile}
        onOpenChange={(open) => {
          if (!open) setPendingFile(null);
        }}
      />
    </div>
  );
}

function NewProductDocumentDialog({
  productId,
  file,
  onOpenChange,
}: {
  productId: string;
  file: File | null;
  onOpenChange: (open: boolean) => void;
}): JSX.Element {
  const upload = useUploadProductDocument(productId);
  const [title, setTitle] = useState("");
  const [docType, setDocType] = useState<string>("manual");
  const [notes, setNotes] = useState("");

  const open = file !== null;
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
      },
      {
        onSuccess: () => {
          setTitle("");
          setDocType("manual");
          setNotes("");
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
            <Label htmlFor="product-doc-title">Title</Label>
            <Input
              id="product-doc-title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              autoFocus
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="product-doc-type">Type</Label>
            <Select value={docType} onValueChange={setDocType}>
              <SelectTrigger id="product-doc-type">
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
            <Label htmlFor="product-doc-notes">Notes</Label>
            <Textarea
              id="product-doc-notes"
              rows={2}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
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
            <Button type="submit" disabled={upload.isPending || !title.trim()}>
              {upload.isPending ? "Uploading…" : "Add"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
