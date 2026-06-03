/**
 * Product manuals tab (§7.4).
 *
 * Two surfaces, switched on `?manual=<uuid>`:
 *   - Manual list   : cards + "New manual" dialog.
 *   - Manual editor : split-pane markdown textarea + live preview, a
 *                     revision sidebar (revision string + status
 *                     badge), and Save-draft / Publish actions.
 *                     Older revisions render read-only with their
 *                     change note.
 *
 * Publishing supersedes the prior published revision server-side; we
 * just invalidate the revision list so the badges flip.
 */

import { useEffect, useMemo, useState } from "react";
import {
  ArrowLeftIcon,
  BookOpenIcon,
  PlusIcon,
} from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
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
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";

import type {
  ManualRevisionDto,
  RevisionStatus,
} from "../api/schemas/products.js";
import { Markdown } from "../components/markdown.jsx";
import { navigate, productManualRoute } from "../routes.js";

import {
  useCreateManualRevision,
  useCreateProductManual,
  useManualRevisions,
  useProductManuals,
  usePublishManualRevision,
} from "./use-products-data.js";

const REV_STATUS_VARIANT: Record<
  RevisionStatus,
  "default" | "secondary" | "outline"
> = {
  published: "default",
  draft: "secondary",
  superseded: "outline",
};

const REV_STATUS_LABEL: Record<RevisionStatus, string> = {
  published: "Published",
  draft: "Draft",
  superseded: "Superseded",
};

export function ProductManualsSection({
  productId,
  activeManualId,
}: {
  productId: string;
  activeManualId: string | null;
}): JSX.Element {
  if (activeManualId) {
    return <ManualEditor productId={productId} manualId={activeManualId} />;
  }
  return <ManualList productId={productId} />;
}

// ---------------------------------------------------------------------------
// Manual list
// ---------------------------------------------------------------------------

function ManualList({ productId }: { productId: string }): JSX.Element {
  const manuals = useProductManuals(productId);
  const [createOpen, setCreateOpen] = useState(false);

  const rows = manuals.data ?? [];

  return (
    <div className="flex flex-col gap-4" data-testid="product-manuals">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">
          Manuals <span className="text-muted-foreground">({rows.length})</span>
        </h3>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setCreateOpen(true)}
          data-testid="product-manual-new"
        >
          <PlusIcon className="mr-1.5 h-4 w-4" /> New manual
        </Button>
      </div>

      {manuals.isError ? (
        <Alert variant="destructive" data-testid="product-manuals-error">
          <AlertTitle>Couldn't load manuals</AlertTitle>
          <AlertDescription>{manuals.error.message}</AlertDescription>
        </Alert>
      ) : manuals.isPending ? (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {Array.from({ length: 2 }).map((_, i) => (
            <Skeleton key={i} className="h-20 rounded-xl" />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <div
          className="flex flex-col items-center gap-3 rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-12 text-center"
          data-testid="product-manuals-empty"
        >
          <BookOpenIcon className="h-7 w-7 text-muted-foreground" />
          <div className="text-sm text-muted-foreground">
            No manuals yet. Create one to start drafting revisions in
            markdown.
          </div>
          <Button size="sm" onClick={() => setCreateOpen(true)}>
            <PlusIcon className="mr-1.5 h-4 w-4" /> New manual
          </Button>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {rows.map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => navigate(productManualRoute(productId, m.id))}
              className="text-left focus:outline-none"
              data-testid="product-manual-card"
            >
              <Card className="gap-2 py-4 transition-colors hover:border-primary/40 hover:bg-accent/20">
                <CardHeader className="px-4">
                  <CardTitle className="truncate text-base" title={m.title}>
                    {m.title}
                  </CardTitle>
                </CardHeader>
                <CardContent className="px-4 text-xs text-muted-foreground">
                  Updated {new Date(m.updated_at).toLocaleDateString("en-AU")}
                </CardContent>
              </Card>
            </button>
          ))}
        </div>
      )}

      <NewManualDialog
        productId={productId}
        open={createOpen}
        onOpenChange={setCreateOpen}
      />
    </div>
  );
}

function NewManualDialog({
  productId,
  open,
  onOpenChange,
}: {
  productId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): JSX.Element {
  const create = useCreateProductManual(productId);
  const [title, setTitle] = useState("");

  useEffect(() => {
    if (open) {
      setTitle("");
      create.reset();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!title.trim()) return;
    create.mutate(
      { title: title.trim() },
      {
        onSuccess: (manual) => {
          onOpenChange(false);
          navigate(productManualRoute(productId, manual.id));
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md" data-testid="new-manual-dialog">
        <DialogHeader>
          <DialogTitle>New manual</DialogTitle>
          <DialogDescription>
            Give the manual a title. You'll add revisions (markdown
            bodies) inside the editor.
          </DialogDescription>
        </DialogHeader>
        <form className="flex flex-col gap-3" onSubmit={onSubmit}>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="new-manual-title">Title</Label>
            <Input
              id="new-manual-title"
              data-testid="new-manual-title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Installation guide"
              autoFocus
            />
          </div>
          {create.isError && (
            <Alert variant="destructive">
              <AlertDescription>{create.error.message}</AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={create.isPending}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              data-testid="new-manual-submit"
              disabled={create.isPending || !title.trim()}
            >
              {create.isPending ? "Creating…" : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

// ---------------------------------------------------------------------------
// Manual editor
// ---------------------------------------------------------------------------

function ManualEditor({
  productId,
  manualId,
}: {
  productId: string;
  manualId: string;
}): JSX.Element {
  const manuals = useProductManuals(productId);
  const revisions = useManualRevisions(productId, manualId);
  const manual = (manuals.data ?? []).find((m) => m.id === manualId);

  const rows = useMemo(() => revisions.data ?? [], [revisions.data]);

  // Selected revision in the sidebar. Defaults to the newest (rows are
  // returned newest-first by convention; we sort defensively below).
  const sorted = useMemo(
    () =>
      [...rows].sort(
        (a, b) =>
          new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
      ),
    [rows],
  );
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected =
    sorted.find((r) => r.id === selectedId) ?? sorted[0] ?? null;

  return (
    <div className="flex flex-col gap-4" data-testid="manual-editor">
      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => navigate(productManualRoute(productId, null))}
          data-testid="manual-editor-back"
        >
          <ArrowLeftIcon className="mr-1.5 h-4 w-4" /> Manuals
        </Button>
        <h3 className="text-sm font-medium">
          {manual ? manual.title : "Manual"}
        </h3>
      </div>

      {revisions.isError ? (
        <Alert variant="destructive" data-testid="manual-revisions-error">
          <AlertTitle>Couldn't load revisions</AlertTitle>
          <AlertDescription>{revisions.error.message}</AlertDescription>
        </Alert>
      ) : revisions.isPending ? (
        <Skeleton className="h-64 rounded-xl" />
      ) : (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-[220px_1fr]">
          <RevisionSidebar
            revisions={sorted}
            selectedId={selected?.id ?? null}
            onSelect={setSelectedId}
            onNewDraft={() => setSelectedId("__new__")}
          />
          <div className="min-w-0">
            {selectedId === "__new__" || sorted.length === 0 ? (
              <NewRevisionForm
                productId={productId}
                manualId={manualId}
                latestRevisionLabel={sorted[0]?.revision ?? null}
                onSaved={(rev) => setSelectedId(rev.id)}
              />
            ) : selected ? (
              <RevisionView
                productId={productId}
                manualId={manualId}
                revision={selected}
              />
            ) : null}
          </div>
        </div>
      )}
    </div>
  );
}

function RevisionSidebar({
  revisions,
  selectedId,
  onSelect,
  onNewDraft,
}: {
  revisions: ManualRevisionDto[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onNewDraft: () => void;
}): JSX.Element {
  return (
    <aside className="flex flex-col gap-2" data-testid="manual-revision-sidebar">
      <Button
        variant="outline"
        size="sm"
        onClick={onNewDraft}
        data-testid="manual-new-revision"
      >
        <PlusIcon className="mr-1.5 h-4 w-4" /> New revision
      </Button>
      {revisions.length === 0 ? (
        <p className="px-1 text-xs text-muted-foreground">
          No revisions yet.
        </p>
      ) : (
        <ul className="flex flex-col gap-1">
          {revisions.map((r) => (
            <li key={r.id}>
              <button
                type="button"
                onClick={() => onSelect(r.id)}
                className={cn(
                  "flex w-full items-center justify-between gap-2 rounded-md border px-2.5 py-1.5 text-left text-xs",
                  selectedId === r.id
                    ? "border-primary bg-accent/40"
                    : "border-border hover:bg-accent/20",
                )}
                data-testid="manual-revision-item"
              >
                <span className="truncate font-mono">{r.revision}</span>
                <Badge
                  variant={REV_STATUS_VARIANT[r.status]}
                  className="px-1.5 py-0 text-[10px]"
                >
                  {REV_STATUS_LABEL[r.status]}
                </Badge>
              </button>
            </li>
          ))}
        </ul>
      )}
    </aside>
  );
}

function RevisionView({
  productId,
  manualId,
  revision,
}: {
  productId: string;
  manualId: string;
  revision: ManualRevisionDto;
}): JSX.Element {
  const publish = usePublishManualRevision(productId, manualId);
  const canPublish = revision.status === "draft";

  return (
    <div className="flex flex-col gap-3" data-testid="manual-revision-view">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <span className="font-mono text-sm font-medium">
            {revision.revision}
          </span>
          <Badge variant={REV_STATUS_VARIANT[revision.status]}>
            {REV_STATUS_LABEL[revision.status]}
          </Badge>
          <span className="text-xs text-muted-foreground">
            {new Date(revision.created_at).toLocaleString("en-AU")}
          </span>
        </div>
        {canPublish && (
          <Button
            size="sm"
            onClick={() => publish.mutate(revision.id)}
            disabled={publish.isPending}
            data-testid="manual-revision-publish"
          >
            {publish.isPending ? "Publishing…" : "Publish"}
          </Button>
        )}
      </div>

      {revision.change_note && (
        <p className="rounded-md bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
          <span className="font-medium">Change note:</span>{" "}
          {revision.change_note}
        </p>
      )}

      {publish.isError && (
        <Alert variant="destructive">
          <AlertDescription>{publish.error.message}</AlertDescription>
        </Alert>
      )}

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <div className="min-w-0">
          <Label className="mb-1.5 block text-xs text-muted-foreground">
            Source
          </Label>
          <Textarea
            value={revision.body_md}
            readOnly
            rows={18}
            className="font-mono text-xs"
            data-testid="manual-revision-source"
          />
        </div>
        <div className="min-w-0">
          <Label className="mb-1.5 block text-xs text-muted-foreground">
            Preview
          </Label>
          <div className="min-h-[18rem] rounded-md border bg-background p-3">
            <Markdown>{revision.body_md || "_Empty revision._"}</Markdown>
          </div>
        </div>
      </div>
    </div>
  );
}

function NewRevisionForm({
  productId,
  manualId,
  latestRevisionLabel,
  onSaved,
}: {
  productId: string;
  manualId: string;
  latestRevisionLabel: string | null;
  onSaved: (rev: ManualRevisionDto) => void;
}): JSX.Element {
  const create = useCreateManualRevision(productId, manualId);
  const [revision, setRevision] = useState("");
  const [bodyMd, setBodyMd] = useState("");
  const [changeNote, setChangeNote] = useState("");

  const canSubmit = revision.trim().length > 0 && !create.isPending;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!canSubmit) return;
    create.mutate(
      {
        revision: revision.trim(),
        body_md: bodyMd,
        change_note: changeNote.trim() || undefined,
      },
      {
        onSuccess: (rev) => {
          setRevision("");
          setBodyMd("");
          setChangeNote("");
          onSaved(rev);
        },
      },
    );
  };

  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={onSubmit}
      data-testid="manual-new-revision-form"
    >
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="revision-label">Revision</Label>
          <Input
            id="revision-label"
            data-testid="revision-label"
            value={revision}
            onChange={(e) => setRevision(e.target.value)}
            placeholder={latestRevisionLabel ? `e.g. after ${latestRevisionLabel}` : "e.g. A or 1.0"}
            autoFocus
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="revision-note">Change note</Label>
          <Input
            id="revision-note"
            data-testid="revision-note"
            value={changeNote}
            onChange={(e) => setChangeNote(e.target.value)}
            placeholder="What changed?"
          />
        </div>
      </div>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <div className="min-w-0">
          <Label
            htmlFor="revision-body"
            className="mb-1.5 block text-xs text-muted-foreground"
          >
            Markdown
          </Label>
          <Textarea
            id="revision-body"
            value={bodyMd}
            onChange={(e) => setBodyMd(e.target.value)}
            rows={18}
            className="font-mono text-xs"
            placeholder="# Heading&#10;&#10;Write the manual body in markdown…"
            data-testid="revision-body"
          />
        </div>
        <div className="min-w-0">
          <Label className="mb-1.5 block text-xs text-muted-foreground">
            Preview
          </Label>
          <div className="min-h-[18rem] rounded-md border bg-background p-3">
            <Markdown>{bodyMd || "_Nothing to preview yet._"}</Markdown>
          </div>
        </div>
      </div>

      {create.isError && (
        <Alert variant="destructive">
          <AlertDescription>{create.error.message}</AlertDescription>
        </Alert>
      )}

      <div className="flex justify-end">
        <Button
          type="submit"
          disabled={!canSubmit}
          data-testid="revision-save-draft"
        >
          {create.isPending ? "Saving…" : "Save draft"}
        </Button>
      </div>
    </form>
  );
}
