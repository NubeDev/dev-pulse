/**
 * Product detail → Firmware & Software releases tab.
 *
 * Displays two groups (Firmware, Software) with a table of release rows
 * per group. Supports creating, editing (full CAS PATCH upsert), and
 * archiving (soft-delete) releases.
 */

import { useEffect, useState } from "react";
import {
  ExternalLinkIcon,
  PencilIcon,
  PlusIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";

import { Markdown } from "../../components/markdown.jsx";
import type {
  ArchiveReleaseRequest,
  CreateReleaseRequest,
  PatchReleaseRequest,
  ProductDto,
  ProductReleaseDto,
  ReleaseKind,
  ReleaseLink,
} from "../../api/schemas/products.js";
import {
  useArchiveRelease,
  useCreateRelease,
  usePatchRelease,
  useProductReleases,
} from "../use-products-data.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function sortReleases(releases: ProductReleaseDto[]): ProductReleaseDto[] {
  return [...releases].sort((a, b) => {
    if (b.major !== a.major) return b.major - a.major;
    return b.minor - a.minor;
  });
}

function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleDateString("en-AU", {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  } catch {
    return iso;
  }
}

// ---------------------------------------------------------------------------
// Main section
// ---------------------------------------------------------------------------

export function ProductReleasesSection({
  product,
}: {
  product: ProductDto;
}): JSX.Element {
  const releases = useProductReleases(product.id);
  const [newOpen, setNewOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<ProductReleaseDto | null>(null);
  const [archiveTarget, setArchiveTarget] = useState<ProductReleaseDto | null>(
    null,
  );

  const archiveMutation = useArchiveRelease(product.id);

  const allRows = releases.data ?? [];
  const firmware = sortReleases(allRows.filter((r) => r.kind === "firmware"));
  const software = sortReleases(allRows.filter((r) => r.kind === "software"));

  const onArchiveConfirm = (): void => {
    if (!archiveTarget) return;
    const body: ArchiveReleaseRequest = {
      expected_version: archiveTarget.version,
    };
    archiveMutation.mutate(
      { releaseId: archiveTarget.id, body },
      { onSuccess: () => setArchiveTarget(null) },
    );
  };

  return (
    <div className="flex flex-col gap-4" data-testid="product-releases">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">
          Firmware &amp; Software releases{" "}
          <span className="text-muted-foreground">({allRows.length})</span>
        </h3>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setNewOpen(true)}
          data-testid="new-release-button"
        >
          <PlusIcon className="mr-1.5 h-4 w-4" /> New release
        </Button>
      </div>

      {releases.isError ? (
        <Alert variant="destructive" data-testid="product-releases-error">
          <AlertTitle>Couldn't load releases</AlertTitle>
          <AlertDescription>{releases.error.message}</AlertDescription>
        </Alert>
      ) : releases.isPending ? (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 3 }).map((_, i) => (
            <Skeleton key={i} className="h-11 rounded-md" />
          ))}
        </div>
      ) : allRows.length === 0 ? (
        <div
          className="rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground"
          data-testid="product-releases-empty"
        >
          No releases yet.{" "}
          <button
            type="button"
            className="underline"
            onClick={() => setNewOpen(true)}
          >
            Add the first release
          </button>{" "}
          to track firmware and software versions for this product.
        </div>
      ) : (
        <div className="flex flex-col gap-4">
          <ReleaseGroup
            title="Firmware"
            kind="firmware"
            rows={firmware}
            testId="releases-group-firmware"
            onEdit={setEditTarget}
            onArchive={setArchiveTarget}
          />
          <ReleaseGroup
            title="Software"
            kind="software"
            rows={software}
            testId="releases-group-software"
            onEdit={setEditTarget}
            onArchive={setArchiveTarget}
          />
        </div>
      )}

      {/* Create dialog */}
      <NewReleaseDialog
        open={newOpen}
        onOpenChange={setNewOpen}
        product={product}
      />

      {/* Edit dialog */}
      {editTarget && (
        <EditReleaseDialog
          open={!!editTarget}
          onOpenChange={(open) => {
            if (!open) setEditTarget(null);
          }}
          product={product}
          release={editTarget}
        />
      )}

      {/* Archive confirm */}
      <AlertDialog
        open={!!archiveTarget}
        onOpenChange={(open) => {
          if (!open) setArchiveTarget(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Archive {archiveTarget?.version_label}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              This release will be hidden from the list. The record is
              preserved and can be recovered via the API.
            </AlertDialogDescription>
          </AlertDialogHeader>
          {archiveMutation.isError && (
            <Alert variant="destructive">
              <AlertDescription>
                {archiveMutation.error.message}
              </AlertDescription>
            </Alert>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={archiveMutation.isPending}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={onArchiveConfirm}
              disabled={archiveMutation.isPending}
            >
              {archiveMutation.isPending ? "Archiving…" : "Archive"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Release group (Firmware or Software)
// ---------------------------------------------------------------------------

function ReleaseGroup({
  title,
  kind,
  rows,
  testId,
  onEdit,
  onArchive,
}: {
  title: string;
  kind: ReleaseKind;
  rows: ProductReleaseDto[];
  testId: string;
  onEdit: (r: ProductReleaseDto) => void;
  onArchive: (r: ProductReleaseDto) => void;
}): JSX.Element {
  return (
    <Card data-testid={testId}>
      <CardHeader className="pb-2 pt-4 px-4">
        <CardTitle className="flex items-center gap-2 text-sm">
          {title}
          <Badge variant={kind === "firmware" ? "default" : "secondary"}>
            {kind}
          </Badge>
          <span className="text-muted-foreground font-normal">
            ({rows.length})
          </span>
        </CardTitle>
      </CardHeader>
      <CardContent className="px-0 pb-0">
        {rows.length === 0 ? (
          <p className="px-4 pb-4 text-sm text-muted-foreground">
            No {title.toLowerCase()} releases yet.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="pl-4">Version</TableHead>
                <TableHead>Released</TableHead>
                <TableHead>Notes</TableHead>
                <TableHead className="w-20 text-right pr-4">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((r) => (
                <TableRow
                  key={r.id}
                  data-testid={`release-row-${r.id}`}
                >
                  <TableCell className="pl-4">
                    <span className="font-mono text-sm">
                      {r.version_label}
                    </span>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground whitespace-nowrap">
                    {formatDate(r.released_at)}
                  </TableCell>
                  <TableCell className="text-sm max-w-sm">
                    {r.release_notes ? (
                      <div className="line-clamp-3">
                        <Markdown>{r.release_notes}</Markdown>
                      </div>
                    ) : !r.links?.length ? (
                      <span className="text-muted-foreground">—</span>
                    ) : null}
                    {r.links?.length ? (
                      <div className="mt-1.5 flex flex-wrap gap-1.5">
                        {r.links.map((l, i) => (
                          <a
                            key={i}
                            href={l.url}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="inline-flex max-w-[16rem] items-center gap-1 rounded-md border border-border bg-muted/40 px-2 py-0.5 text-xs text-foreground hover:bg-accent/40"
                            title={l.url}
                            data-testid="release-link-chip"
                          >
                            <ExternalLinkIcon className="h-3 w-3 shrink-0 text-muted-foreground" />
                            <span className="truncate">{l.label || l.url}</span>
                          </a>
                        ))}
                      </div>
                    ) : null}
                  </TableCell>
                  <TableCell className="text-right pr-4">
                    <div className="flex items-center justify-end gap-1">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 text-muted-foreground hover:text-foreground"
                        title="Edit release"
                        onClick={() => onEdit(r)}
                      >
                        <PencilIcon className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 text-muted-foreground hover:text-destructive"
                        title="Archive release"
                        onClick={() => onArchive(r)}
                      >
                        <Trash2Icon className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}

// ---------------------------------------------------------------------------
// Shared form state type
// ---------------------------------------------------------------------------

type ReleaseFormState = {
  kind: ReleaseKind;
  major: string;
  minor: string;
  releasedAt: string;
  notes: string;
  links: ReleaseLink[];
};

function defaultForm(overrides?: Partial<ReleaseFormState>): ReleaseFormState {
  return {
    kind: "firmware",
    major: "1",
    minor: "0",
    releasedAt: "",
    notes: "",
    links: [],
    ...overrides,
  };
}

/** Drop blank rows so empty link inputs aren't submitted. */
function cleanLinks(links: ReleaseLink[]): ReleaseLink[] {
  return links
    .map((l) => ({ label: l.label.trim(), url: l.url.trim() }))
    .filter((l) => l.url.length > 0);
}

function parseVersion(s: string): number | null {
  const n = parseInt(s, 10);
  if (Number.isNaN(n) || n < 0 || !Number.isInteger(n)) return null;
  return n;
}

function versionPreview(major: string, minor: string): string {
  const ma = parseInt(major, 10);
  const mi = parseInt(minor, 10);
  if (Number.isNaN(ma) || Number.isNaN(mi)) return "v?.?";
  return `v${ma}.${mi}`;
}

// ---------------------------------------------------------------------------
// New release dialog
// ---------------------------------------------------------------------------

function NewReleaseDialog({
  open,
  onOpenChange,
  product,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  product: ProductDto;
}): JSX.Element {
  const create = useCreateRelease(product.id);
  const [form, setForm] = useState<ReleaseFormState>(defaultForm);

  useEffect(() => {
    if (!open) return;
    setForm(defaultForm());
    create.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const majorNum = parseVersion(form.major);
  const minorNum = parseVersion(form.minor);
  const canSubmit =
    majorNum !== null && minorNum !== null && !create.isPending;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!canSubmit || majorNum === null || minorNum === null) return;
    const body: CreateReleaseRequest = {
      kind: form.kind,
      major: majorNum,
      minor: minorNum,
      release_notes: form.notes.trim() || null,
      released_at: form.releasedAt.trim() || null,
      links: cleanLinks(form.links),
    };
    create.mutate(body, {
      onSuccess: () => onOpenChange(false),
    });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-lg"
        data-testid="new-release-dialog"
      >
        <DialogHeader>
          <DialogTitle>New release</DialogTitle>
          <DialogDescription>
            Add a firmware or software release for {product.name}.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
          <ReleaseFormFields form={form} onChange={setForm} />

          {create.isError && (
            <Alert variant="destructive">
              <AlertTitle>Create failed</AlertTitle>
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
              data-testid="new-release-submit"
              disabled={!canSubmit}
            >
              {create.isPending ? "Creating…" : "Create release"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

// ---------------------------------------------------------------------------
// Edit release dialog
// ---------------------------------------------------------------------------

function EditReleaseDialog({
  open,
  onOpenChange,
  product,
  release,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  product: ProductDto;
  release: ProductReleaseDto;
}): JSX.Element {
  const patch = usePatchRelease(product.id);

  // Seed form from release on open
  const seedForm = (): ReleaseFormState => ({
    kind: release.kind,
    major: String(release.major),
    minor: String(release.minor),
    releasedAt: release.released_at
      ? release.released_at.slice(0, 10)
      : "",
    notes: release.release_notes ?? "",
    links: release.links ?? [],
  });

  const [form, setForm] = useState<ReleaseFormState>(seedForm);

  useEffect(() => {
    if (!open) return;
    setForm(seedForm());
    patch.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, release.id]);

  const majorNum = parseVersion(form.major);
  const minorNum = parseVersion(form.minor);
  const canSubmit =
    majorNum !== null && minorNum !== null && !patch.isPending;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!canSubmit || majorNum === null || minorNum === null) return;
    const body: PatchReleaseRequest = {
      expected_version: release.version,
      kind: form.kind,
      major: majorNum,
      minor: minorNum,
      release_notes: form.notes.trim() || null,
      released_at: form.releasedAt.trim() || null,
      links: cleanLinks(form.links),
    };
    patch.mutate(
      { releaseId: release.id, body },
      { onSuccess: () => onOpenChange(false) },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Edit release</DialogTitle>
          <DialogDescription>
            Update {release.version_label} for {product.name}. Saves
            under CAS — a concurrent edit surfaces as a stale-version
            error.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
          <ReleaseFormFields form={form} onChange={setForm} />

          {patch.isError && (
            <Alert variant="destructive">
              <AlertTitle>Save failed</AlertTitle>
              <AlertDescription>{patch.error.message}</AlertDescription>
            </Alert>
          )}

          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={patch.isPending}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={!canSubmit}>
              {patch.isPending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

// ---------------------------------------------------------------------------
// Shared form fields component
// ---------------------------------------------------------------------------

function ReleaseFormFields({
  form,
  onChange,
}: {
  form: ReleaseFormState;
  onChange: (next: ReleaseFormState) => void;
}): JSX.Element {
  const set =
    <K extends keyof ReleaseFormState>(key: K) =>
    (value: ReleaseFormState[K]) =>
      onChange({ ...form, [key]: value });

  const majorErr =
    parseVersion(form.major) === null
      ? "Must be a non-negative integer."
      : null;
  const minorErr =
    parseVersion(form.minor) === null
      ? "Must be a non-negative integer."
      : null;

  return (
    <>
      <div className="flex flex-col gap-2">
        <Label htmlFor="release-kind">Kind</Label>
        <Select
          value={form.kind}
          onValueChange={(v) => set("kind")(v as ReleaseKind)}
        >
          <SelectTrigger id="release-kind" data-testid="release-kind">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="firmware">Firmware</SelectItem>
            <SelectItem value="software">Software</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <div className="flex flex-col gap-2">
          <Label htmlFor="release-major">Major</Label>
          <Input
            id="release-major"
            data-testid="release-major"
            type="number"
            min={0}
            step={1}
            value={form.major}
            onChange={(e) => set("major")(e.target.value)}
          />
          {majorErr && (
            <p className="text-xs text-destructive">{majorErr}</p>
          )}
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="release-minor">Minor</Label>
          <Input
            id="release-minor"
            data-testid="release-minor"
            type="number"
            min={0}
            step={1}
            value={form.minor}
            onChange={(e) => set("minor")(e.target.value)}
          />
          {minorErr && (
            <p className="text-xs text-destructive">{minorErr}</p>
          )}
        </div>
      </div>

      {/* Live version preview */}
      <div className="rounded-md bg-muted/40 px-3 py-2 text-xs">
        <span className="text-muted-foreground">Version preview:</span>{" "}
        <span className="font-mono font-medium">
          {versionPreview(form.major, form.minor)}
        </span>
        <span className="ml-2 text-muted-foreground">({form.kind})</span>
      </div>

      <div className="flex flex-col gap-2">
        <Label htmlFor="release-date">Released on</Label>
        <Input
          id="release-date"
          type="date"
          lang="en-AU"
          value={form.releasedAt}
          onChange={(e) => set("releasedAt")(e.target.value)}
        />
      </div>

      <div className="flex flex-col gap-2">
        <Label htmlFor="release-notes">Release notes (markdown)</Label>
        <Textarea
          id="release-notes"
          rows={5}
          value={form.notes}
          onChange={(e) => set("notes")(e.target.value)}
          placeholder="## What's new&#10;&#10;- Fix for …&#10;- Improvement in …"
        />
      </div>

      {/* Build / download links */}
      <div className="flex flex-col gap-2">
        <Label>Build links</Label>
        <p className="text-xs text-muted-foreground">
          Links to the firmware/software builds, downloads, or release pages.
        </p>
        {form.links.length > 0 ? (
          <div className="flex flex-col gap-2">
            {form.links.map((l, i) => (
              <div key={i} className="flex items-center gap-2">
                <Input
                  placeholder="Label (e.g. Firmware .bin)"
                  value={l.label}
                  onChange={(e) => {
                    const next = form.links.slice();
                    next[i] = { ...l, label: e.target.value };
                    set("links")(next);
                  }}
                  className="w-44 shrink-0"
                  data-testid="release-link-label"
                />
                <Input
                  placeholder="https://…"
                  value={l.url}
                  onChange={(e) => {
                    const next = form.links.slice();
                    next[i] = { ...l, url: e.target.value };
                    set("links")(next);
                  }}
                  className="flex-1"
                  data-testid="release-link-url"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0 text-muted-foreground hover:text-destructive"
                  onClick={() =>
                    set("links")(form.links.filter((_, j) => j !== i))
                  }
                  title="Remove link"
                  data-testid="release-link-remove"
                >
                  <XIcon className="h-4 w-4" />
                </Button>
              </div>
            ))}
          </div>
        ) : null}
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="self-start"
          onClick={() => set("links")([...form.links, { label: "", url: "" }])}
          data-testid="release-link-add"
        >
          <PlusIcon className="mr-1.5 h-4 w-4" /> Add link
        </Button>
      </div>
    </>
  );
}
