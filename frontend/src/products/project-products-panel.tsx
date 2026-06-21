/**
 * Products panel for the project detail page (§7.4 — project side of
 * the product↔project link). Lists products linked to a project with
 * link / unlink and an "Add product" picker. Kept deliberately small
 * so it can sit as a card inside the project workbench without
 * crowding the issue list.
 */

import { useMemo, useState } from "react";

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
import { Skeleton } from "@/components/ui/skeleton";
import { PackageIcon, PlusIcon, XIcon } from "lucide-react";

import { navigate, productDetailRoute } from "../routes.js";

import { ProductKindBadge } from "./product-list.js";
import {
  useLinkProductProject,
  useProducts,
  useProjectProducts,
  useUnlinkProductProject,
} from "./use-products-data.js";

export function ProjectProductsPanel({
  projectId,
  orgId,
}: {
  projectId: string;
  orgId: string;
}): JSX.Element {
  const linked = useProjectProducts(projectId);
  const unlink = useUnlinkProductProject();
  const [pickerOpen, setPickerOpen] = useState(false);

  const rows = linked.data ?? [];

  return (
    <Card className="gap-2 py-4" data-testid="project-products-panel">
      <CardHeader className="px-4">
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2 text-sm">
            <PackageIcon className="h-4 w-4 text-muted-foreground" />
            Products
            <span className="text-muted-foreground">({rows.length})</span>
          </CardTitle>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setPickerOpen(true)}
            data-testid="project-product-add"
          >
            <PlusIcon className="mr-1.5 h-4 w-4" /> Add product
          </Button>
        </div>
      </CardHeader>
      <CardContent className="px-4">
        {linked.isError ? (
          <p className="text-sm text-destructive">{linked.error.message}</p>
        ) : linked.isPending ? (
          <Skeleton className="h-9 rounded-md" />
        ) : rows.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No products linked to this project yet.
          </p>
        ) : (
          <ul className="flex flex-col gap-1.5">
            {rows.map((p) => (
              <li
                key={p.id}
                className="flex items-center gap-2 rounded-md border border-border px-2.5 py-1.5 text-sm"
                data-testid="project-product-row"
              >
                <button
                  type="button"
                  onClick={() => navigate(productDetailRoute(p.id))}
                  className="min-w-0 flex-1 truncate text-left hover:underline"
                  title={p.name}
                >
                  {p.name}
                </button>
                <span className="shrink-0 font-mono text-xs text-muted-foreground">
                  {p.model_number}
                </span>
                <ProductKindBadge kind={p.kind} />
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-6"
                  onClick={() =>
                    unlink.mutate({ productId: p.id, projectId })
                  }
                  disabled={unlink.isPending}
                  title="Unlink"
                  data-testid="project-product-unlink"
                >
                  <XIcon className="h-3.5 w-3.5" />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </CardContent>

      <ProductPickerDialog
        open={pickerOpen}
        onOpenChange={setPickerOpen}
        orgId={orgId}
        projectId={projectId}
        linkedIds={new Set(rows.map((p) => p.id))}
      />
    </Card>
  );
}

/**
 * Project ▸ Settings ▸ "Manage products…" dialog. Self-contained
 * (linked list with unlink + inline add-search in one dialog, so it
 * can be opened from the settings dropdown without nesting dialogs).
 */
export function ManageProductsDialog({
  open,
  onOpenChange,
  projectId,
  orgId,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  projectId: string;
  orgId: string;
}): JSX.Element {
  const linked = useProjectProducts(projectId);
  const unlink = useUnlinkProductProject();
  const link = useLinkProductProject();
  const [search, setSearch] = useState("");

  const rows = linked.data ?? [];
  const linkedIds = useMemo(() => new Set(rows.map((p) => p.id)), [rows]);
  const productsQ = useProducts({
    org_id: orgId,
    q: search.trim() || undefined,
    limit: 100,
  });
  const candidates = useMemo(
    () => (productsQ.data?.rows ?? []).filter((p) => !linkedIds.has(p.id)),
    [productsQ.data, linkedIds],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg" data-testid="project-manage-products">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <PackageIcon className="h-4 w-4 text-muted-foreground" />
            Products
            <span className="text-muted-foreground">({rows.length})</span>
          </DialogTitle>
          <DialogDescription>
            Link products in this org to the project.
          </DialogDescription>
        </DialogHeader>

        {/* Linked products */}
        {linked.isError ? (
          <p className="text-sm text-destructive">{linked.error.message}</p>
        ) : linked.isPending ? (
          <Skeleton className="h-9 rounded-md" />
        ) : rows.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No products linked to this project yet.
          </p>
        ) : (
          <ul className="flex flex-col gap-1.5">
            {rows.map((p) => (
              <li
                key={p.id}
                className="flex items-center gap-2 rounded-md border border-border px-2.5 py-1.5 text-sm"
                data-testid="project-product-row"
              >
                <button
                  type="button"
                  onClick={() => {
                    onOpenChange(false);
                    navigate(productDetailRoute(p.id));
                  }}
                  className="min-w-0 flex-1 truncate text-left hover:underline"
                  title={p.name}
                >
                  {p.name}
                </button>
                <span className="shrink-0 font-mono text-xs text-muted-foreground">
                  {p.model_number}
                </span>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-6"
                  onClick={() => unlink.mutate({ productId: p.id, projectId })}
                  disabled={unlink.isPending}
                  title="Unlink"
                  data-testid="project-product-unlink"
                >
                  <XIcon className="h-3.5 w-3.5" />
                </Button>
              </li>
            ))}
          </ul>
        )}

        {/* Add a product */}
        <div className="mt-1 border-t border-border pt-3">
          <p className="mb-2 text-xs font-medium text-muted-foreground">
            Add a product
          </p>
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search products…"
            data-testid="project-product-search"
          />
          <div className="mt-2 max-h-56 overflow-y-auto">
            {productsQ.isPending ? (
              <div className="flex flex-col gap-2 py-2">
                {Array.from({ length: 3 }).map((_, i) => (
                  <Skeleton key={i} className="h-9 rounded-md" />
                ))}
              </div>
            ) : productsQ.isError ? (
              <p className="py-3 text-sm text-destructive">
                {productsQ.error.message}
              </p>
            ) : candidates.length === 0 ? (
              <p className="py-4 text-center text-sm text-muted-foreground">
                No more products to link.
              </p>
            ) : (
              <ul className="flex flex-col gap-1 py-1">
                {candidates.map((p) => (
                  <li key={p.id}>
                    <button
                      type="button"
                      onClick={() => link.mutate({ productId: p.id, projectId })}
                      disabled={link.isPending}
                      className="flex w-full items-center justify-between gap-2 rounded-md border border-border px-3 py-2 text-left text-sm hover:bg-accent/30 disabled:opacity-50"
                      data-testid="project-product-option"
                    >
                      <span className="min-w-0 flex-1 truncate">{p.name}</span>
                      <span className="shrink-0 font-mono text-xs text-muted-foreground">
                        {p.model_number}
                      </span>
                      <PlusIcon className="h-4 w-4 shrink-0 text-muted-foreground" />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function ProductPickerDialog({
  open,
  onOpenChange,
  orgId,
  projectId,
  linkedIds,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  orgId: string;
  projectId: string;
  linkedIds: Set<string>;
}): JSX.Element {
  const [search, setSearch] = useState("");
  const link = useLinkProductProject();
  const productsQ = useProducts({
    org_id: orgId,
    q: search.trim() || undefined,
    limit: 100,
  });

  const candidates = useMemo(
    () => (productsQ.data?.rows ?? []).filter((p) => !linkedIds.has(p.id)),
    [productsQ.data, linkedIds],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-md"
        data-testid="project-product-picker"
      >
        <DialogHeader>
          <DialogTitle>Link a product</DialogTitle>
          <DialogDescription>
            Pick a product in the same org to link to this project.
          </DialogDescription>
        </DialogHeader>

        <Input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search products…"
          data-testid="project-product-search"
        />

        <div className="max-h-72 overflow-y-auto">
          {productsQ.isPending ? (
            <div className="flex flex-col gap-2 py-2">
              {Array.from({ length: 4 }).map((_, i) => (
                <Skeleton key={i} className="h-9 rounded-md" />
              ))}
            </div>
          ) : productsQ.isError ? (
            <p className="py-4 text-sm text-destructive">
              {productsQ.error.message}
            </p>
          ) : candidates.length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">
              No more products to link.
            </p>
          ) : (
            <ul className="flex flex-col gap-1 py-1">
              {candidates.map((p) => (
                <li key={p.id}>
                  <button
                    type="button"
                    onClick={() =>
                      link.mutate(
                        { productId: p.id, projectId },
                        { onSuccess: () => onOpenChange(false) },
                      )
                    }
                    disabled={link.isPending}
                    className="flex w-full items-center justify-between gap-2 rounded-md border border-border px-3 py-2 text-left text-sm hover:bg-accent/30 disabled:opacity-50"
                    data-testid="project-product-option"
                  >
                    <span className="min-w-0 flex-1 truncate">{p.name}</span>
                    <span className="shrink-0 font-mono text-xs text-muted-foreground">
                      {p.model_number}
                    </span>
                    <PlusIcon className="h-4 w-4 shrink-0 text-muted-foreground" />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
