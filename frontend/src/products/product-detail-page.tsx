/**
 * `#/products/{id}` — single product detail page (§7.4).
 *
 * Tabbed (shadcn Tabs, `?tab=` persisted): Overview · Projects ·
 * Manuals · Documents. Mirrors the §6.3 project detail page's shape:
 * `PageHeading` with name + status pill, loading / error / not-found
 * states, and an edit-via-PATCH-with-`expected_version` flow.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { PencilIcon, Trash2Icon } from "lucide-react";

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
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs";
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

import { api } from "../api/client.js";
import type {
  PatchProductRequest,
  ProductDto,
  ProductKind,
  ProductStatus,
} from "../api/schemas/products.js";
import { Markdown } from "../components/markdown.jsx";
import { PageHeading } from "../components/page-heading.jsx";
import {
  navigate,
  productDetailTab,
  productDetailTabRoute,
  productManualId,
  useRoute,
  type ProductDetailTab,
} from "../routes.js";
import { MarkdownField, TextField } from "./../projects/exec-summary/form-fields.js";

import { KIND_LABEL, KINDS } from "./new-product-dialog.js";
import { ManufacturerField } from "./manufacturer-field.js";
import { PRODUCT_STATUS_VARIANT, ProductKindBadge } from "./product-list.js";
import { ProductDocumentsSection } from "./product-documents-section.js";
import { ProductManualsSection } from "./product-manuals-section.js";
import { ProductProjectsSection } from "./product-projects-section.js";
import { ProductRunsSection } from "./runs/product-runs-section.js";
import { ProductUnitsSection } from "./units/product-units-section.js";
import { ProductReturnsSection } from "./rma/product-returns-section.js";
import { ProductReleasesSection } from "./releases/product-releases-section.js";
import {
  useArchiveProduct,
  usePatchProduct,
  useProduct,
} from "./use-products-data.js";

const STATUS_LABEL: Record<ProductStatus, string> = {
  draft: "Draft",
  active: "Active",
  eol: "EOL",
  archived: "Archived",
};
const STATUSES: ProductStatus[] = ["draft", "active", "eol", "archived"];

/** Build a complete `PatchProductRequest` from the current product so a
 *  partial edit doesn't NULL its sibling fields (the server PATCH is a
 *  full upsert — see `OverviewTab`). `overrides` win. */
function fullPatchBody(
  product: ProductDto,
  overrides: Partial<PatchProductRequest>,
): PatchProductRequest {
  return {
    expected_version: product.version,
    name: product.name,
    model_number: product.model_number,
    status: product.status,
    kind: product.kind,
    description: product.description ?? null,
    manufacturer_id: product.manufacturer_id ?? null,
    serial_prefix: product.serial_prefix ?? null,
    serial_format: product.serial_format ?? null,
    ...overrides,
  };
}

export function ProductDetailPage({
  productId,
}: {
  productId: string;
}): JSX.Element {
  const product = useProduct(productId);

  if (product.isPending) {
    return (
      <div className="px-4 lg:px-6">
        <div
          className="flex items-center gap-2 py-4 text-sm text-muted-foreground"
          data-testid="product-detail-loading"
        >
          <Spinner /> Loading product…
        </div>
      </div>
    );
  }

  if (product.isError) {
    return (
      <div className="px-4 lg:px-6">
        <Alert variant="destructive" data-testid="product-detail-error">
          <AlertTitle>Couldn't load product</AlertTitle>
          <AlertDescription>{product.error.message}</AlertDescription>
        </Alert>
      </div>
    );
  }

  if (!product.data) {
    return (
      <div className="px-4 lg:px-6">
        <Alert data-testid="product-detail-missing">
          <AlertTitle>Product not found</AlertTitle>
          <AlertDescription>
            This product either doesn't exist or you don't have access
            to it.{" "}
            <a className="underline" href="#/products">
              Back to products
            </a>
            .
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  return <ProductDetailBody product={product.data} />;
}

function ProductDetailBody({ product }: { product: ProductDto }): JSX.Element {
  const route = useRoute();
  const activeTab = productDetailTab(route);
  const activeManualId = productManualId(route);
  const [editOpen, setEditOpen] = useState(false);
  const [archiveOpen, setArchiveOpen] = useState(false);

  const archive = useArchiveProduct(product.id);
  const patch = usePatchProduct(product.id);
  const isArchived = product.status === "archived";

  const manufacturer = useQuery({
    queryKey: ["parties", "manufacturers", "detail", product.manufacturer_id],
    queryFn: () =>
      product.manufacturer_id
        ? api.getManufacturer(product.manufacturer_id)
        : Promise.resolve(null),
    enabled: !!product.manufacturer_id,
    staleTime: 60_000,
  });

  const onArchiveConfirm = (): void => {
    if (isArchived) {
      patch.mutate(
        fullPatchBody(product, { status: "active" }),
        { onSuccess: () => setArchiveOpen(false) },
      );
    } else {
      archive.mutate(
        { expected_version: product.version },
        { onSuccess: () => setArchiveOpen(false) },
      );
    }
  };

  return (
    <div className="flex flex-col gap-4 px-4 lg:px-6" data-testid="product-detail">
      <PageHeading
        title={
          <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span data-testid="product-detail-name">{product.name}</span>
            <Button
              variant="ghost"
              size="icon"
              className="size-6 text-muted-foreground hover:text-foreground"
              title="Edit product"
              data-testid="product-detail-edit"
              onClick={() => setEditOpen(true)}
            >
              <PencilIcon className="size-3.5" />
            </Button>
            <Badge
              variant={PRODUCT_STATUS_VARIANT[product.status]}
              data-testid="product-detail-status"
            >
              {STATUS_LABEL[product.status]}
            </Badge>
          </span>
        }
        description={
          <span className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
            <span className="font-mono">{product.model_number}</span>
            {product.manufacturer_id ? (
              <span>· {manufacturer.data?.name ?? "…"}</span>
            ) : null}
            <ProductKindBadge kind={product.kind} />
          </span>
        }
        trailing={
          <Button
            variant="outline"
            size="sm"
            onClick={() => setArchiveOpen(true)}
            data-testid={
              isArchived ? "product-restore-button" : "product-archive-button"
            }
          >
            <Trash2Icon className="mr-1.5 h-4 w-4" />
            {isArchived ? "Restore" : "Archive"}
          </Button>
        }
      />

      <Tabs
        value={activeTab}
        onValueChange={(v) =>
          navigate(productDetailTabRoute(product.id, v as ProductDetailTab))
        }
        data-testid="product-detail-tabs"
      >
        <TabsList>
          <TabsTrigger value="overview" data-testid="product-tab-overview">
            Overview
          </TabsTrigger>
          <TabsTrigger value="projects" data-testid="product-tab-projects">
            Projects
          </TabsTrigger>
          <TabsTrigger value="runs" data-testid="product-tab-runs">
            Runs
          </TabsTrigger>
          <TabsTrigger value="units" data-testid="product-tab-units">
            Units
          </TabsTrigger>
          <TabsTrigger value="releases" data-testid="product-tab-releases">
            Firmware &amp; Software
          </TabsTrigger>
          <TabsTrigger value="manuals" data-testid="product-tab-manuals">
            Manuals
          </TabsTrigger>
          <TabsTrigger value="documents" data-testid="product-tab-documents">
            Documents
          </TabsTrigger>
          <TabsTrigger value="returns" data-testid="product-tab-returns">
            Returns
          </TabsTrigger>
        </TabsList>

        <TabsContent value="overview" className="mt-4">
          <OverviewTab product={product} />
        </TabsContent>
        <TabsContent value="projects" className="mt-4">
          <ProductProjectsSection product={product} />
        </TabsContent>
        <TabsContent value="runs" className="mt-4">
          <ProductRunsSection product={product} />
        </TabsContent>
        <TabsContent value="units" className="mt-4">
          <ProductUnitsSection product={product} />
        </TabsContent>
        <TabsContent value="releases" className="mt-4">
          <ProductReleasesSection product={product} />
        </TabsContent>
        <TabsContent value="manuals" className="mt-4">
          <ProductManualsSection
            productId={product.id}
            activeManualId={activeManualId}
          />
        </TabsContent>
        <TabsContent value="documents" className="mt-4">
          <ProductDocumentsSection productId={product.id} />
        </TabsContent>
        <TabsContent value="returns" className="mt-4">
          <ProductReturnsSection product={product} />
        </TabsContent>
      </Tabs>

      <EditProductDialog
        open={editOpen}
        onOpenChange={setEditOpen}
        product={product}
      />

      <AlertDialog open={archiveOpen} onOpenChange={setArchiveOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {isArchived
                ? `Restore "${product.name}"?`
                : `Archive "${product.name}"?`}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {isArchived
                ? "Move this product back to Active."
                : "Archived products are hidden from the default catalogue but keep their manuals, documents, and links. You can restore later."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={archive.isPending || patch.isPending}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              onClick={onArchiveConfirm}
              disabled={archive.isPending || patch.isPending}
            >
              {isArchived ? "Restore" : "Archive"}
            </AlertDialogAction>
          </AlertDialogFooter>
          {(archive.error || patch.error) && (
            <p className="text-sm text-destructive">
              {(archive.error ?? patch.error)?.message}
            </p>
          )}
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Overview tab — model #, manufacturer, status, markdown description,
// and the serial-format config with a live example-serial preview.
// Each field PATCHes with `expected_version`.
// ---------------------------------------------------------------------------

function OverviewTab({ product }: { product: ProductDto }): JSX.Element {
  const patch = usePatchProduct(product.id);

  // The product PATCH is a FULL upsert — any field omitted from the
  // body is written as NULL server-side. So every commit re-sends the
  // *entire* current product (not just name/model/status) plus the
  // changed field, otherwise editing one field would wipe the others
  // (description, manufacturer, serial config).
  const commit = (overrides: Partial<PatchProductRequest>): void => {
    patch.mutate(fullPatchBody(product, overrides));
  };

  const exampleSerial = buildExampleSerial(
    product.serial_prefix ?? "",
    product.serial_format ?? "",
  );

  return (
    <div className="flex flex-col gap-4" data-testid="product-overview">
      {patch.isError && (
        <Alert variant="destructive" data-testid="product-overview-error">
          <AlertTitle>Save failed</AlertTitle>
          <AlertDescription>{patch.error.message}</AlertDescription>
        </Alert>
      )}

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card className="gap-2 py-4">
          <CardHeader className="px-4">
            <CardTitle className="text-sm">Details</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 px-4">
            <TextField
              label="Model #"
              value={product.model_number}
              onCommit={(next) => {
                const v = (next ?? "").trim();
                if (v.length === 0 || v === product.model_number) return;
                commit({ model_number: v });
              }}
            />
            <div className="flex flex-col gap-1.5">
              <Label className="text-xs font-medium text-muted-foreground">
                Status
              </Label>
              <Select
                value={product.status}
                onValueChange={(v) => {
                  if (v === product.status) return;
                  commit({ status: v as ProductStatus });
                }}
              >
                <SelectTrigger data-testid="product-overview-status">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {STATUSES.map((s) => (
                    <SelectItem key={s} value={s}>
                      {STATUS_LABEL[s]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label className="text-xs font-medium text-muted-foreground">
                Type
              </Label>
              <Select
                value={product.kind}
                onValueChange={(v) => {
                  if (v === product.kind) return;
                  commit({ kind: v as ProductKind });
                }}
              >
                <SelectTrigger data-testid="product-overview-kind">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {KINDS.map((k) => (
                    <SelectItem key={k} value={k}>
                      {KIND_LABEL[k]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <ManufacturerField
              orgId={product.org_id}
              value={product.manufacturer_id ?? null}
              onChange={(id) => {
                if ((product.manufacturer_id ?? null) === id) return;
                commit({ manufacturer_id: id });
              }}
            />
          </CardContent>
        </Card>

        <Card className="gap-2 py-4">
          <CardHeader className="px-4">
            <CardTitle className="text-sm">Serial format</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 px-4">
            <TextField
              label="Serial prefix"
              value={product.serial_prefix ?? ""}
              onCommit={(next) =>
                commit({ serial_prefix: next ?? null })
              }
              placeholder="RX"
            />
            <TextField
              label="Serial format"
              value={product.serial_format ?? ""}
              onCommit={(next) => commit({ serial_format: next ?? null })}
              placeholder="{prefix}-{seq:0000}"
              hint="Tokens: {prefix}, {seq}, {seq:0000} (zero-padded), {year}."
            />
            <div className="rounded-md bg-muted/40 px-3 py-2 text-xs">
              <span className="text-muted-foreground">Example serial:</span>{" "}
              <span className="font-mono" data-testid="product-serial-example">
                {exampleSerial || "—"}
              </span>
            </div>
          </CardContent>
        </Card>
      </div>

      <Card className="gap-2 py-4">
        <CardHeader className="px-4">
          <CardTitle className="text-sm">Description</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-3 px-4">
          <MarkdownField
            label="Description (markdown)"
            value={product.description ?? ""}
            onCommit={(next) => commit({ description: next ?? null })}
          />
          {product.description ? (
            <div className="rounded-md border bg-background p-3">
              <Markdown>{product.description}</Markdown>
            </div>
          ) : null}
        </CardContent>
      </Card>
    </div>
  );
}

/** Render a sample serial from the prefix + format tokens. Best-effort
 *  preview only — the authoritative generator lives server-side (P2). */
function buildExampleSerial(prefix: string, format: string): string {
  const year = String(new Date().getFullYear());
  if (!format) {
    return prefix ? `${prefix}-0001` : "";
  }
  return format
    .replace(/\{prefix\}/g, prefix)
    .replace(/\{year\}/g, year)
    .replace(/\{seq:(0+)\}/g, (_m, pad: string) =>
      String(1).padStart(pad.length, "0"),
    )
    .replace(/\{seq\}/g, "1");
}

// ---------------------------------------------------------------------------
// Edit dialog — name / model # / status. Description + serial config
// live on the Overview tab; this is the quick header edit.
// ---------------------------------------------------------------------------

function EditProductDialog({
  open,
  onOpenChange,
  product,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  product: ProductDto;
}): JSX.Element {
  const patch = usePatchProduct(product.id);
  const [name, setName] = useState(product.name);
  const [modelNumber, setModelNumber] = useState(product.model_number);
  const [status, setStatus] = useState<ProductStatus>(product.status);

  // Re-seed the form whenever the dialog re-opens (handled in the
  // open-change wrapper rather than an effect so the inputs pick up
  // the latest server values without an extra render).
  const onOpen = (next: boolean): void => {
    if (next) {
      setName(product.name);
      setModelNumber(product.model_number);
      setStatus(product.status);
      patch.reset();
    }
    onOpenChange(next);
  };

  const nameError = name.trim().length === 0 ? "Name is required." : null;
  const modelError =
    modelNumber.trim().length === 0 ? "Model # is required." : null;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (nameError || modelError) return;
    patch.mutate(
      fullPatchBody(product, {
        name: name.trim(),
        model_number: modelNumber.trim(),
        status,
      }),
      { onSuccess: () => onOpenChange(false) },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpen}>
      <DialogContent className="sm:max-w-md" data-testid="edit-product-dialog">
        <DialogHeader>
          <DialogTitle>Edit product</DialogTitle>
          <DialogDescription>
            Update the product's name, model #, and status. Saves under
            CAS — a concurrent edit surfaces as a stale-version error.
          </DialogDescription>
        </DialogHeader>
        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
          <div className="flex flex-col gap-2">
            <Label htmlFor="edit-product-name">Name</Label>
            <Input
              id="edit-product-name"
              data-testid="edit-product-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              maxLength={200}
            />
            {nameError && <p className="text-xs text-destructive">{nameError}</p>}
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="edit-product-model">Model #</Label>
            <Input
              id="edit-product-model"
              data-testid="edit-product-model"
              value={modelNumber}
              onChange={(e) => setModelNumber(e.target.value)}
              maxLength={200}
            />
            {modelError && (
              <p className="text-xs text-destructive">{modelError}</p>
            )}
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="edit-product-status">Status</Label>
            <Select
              value={status}
              onValueChange={(v) => setStatus(v as ProductStatus)}
            >
              <SelectTrigger id="edit-product-status">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {STATUSES.map((s) => (
                  <SelectItem key={s} value={s}>
                    {STATUS_LABEL[s]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {patch.isError && (
            <Alert variant="destructive" data-testid="edit-product-error">
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
            <Button
              type="submit"
              data-testid="edit-product-submit"
              disabled={patch.isPending || !!nameError || !!modelError}
            >
              {patch.isPending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Loading skeleton — exported for reuse by the Products pane while a
 *  detail id is resolving. */
export function ProductDetailSkeleton(): JSX.Element {
  return (
    <div className="flex flex-col gap-4 px-4 lg:px-6">
      <Skeleton className="h-10 w-64" />
      <Skeleton className="h-64 rounded-xl" />
    </div>
  );
}
