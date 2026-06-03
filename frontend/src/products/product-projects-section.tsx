/**
 * Product detail → Projects tab (§7.4).
 *
 * Lists the projects linked to a product with link / unlink controls
 * and an "Add" project-picker dialog populated from
 * `api.listProjects` (scoped to the product's org).
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { LinkIcon, PlusIcon, XIcon } from "lucide-react";

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
import { Skeleton } from "@/components/ui/skeleton";

import { api } from "../api/client.js";
import type { ProductDto } from "../api/schemas/products.js";
import { navigate, projectDetailRoute } from "../routes.js";

import {
  useLinkProductProject,
  useProductProjects,
  useUnlinkProductProject,
} from "./use-products-data.js";

export function ProductProjectsSection({
  product,
}: {
  product: ProductDto;
}): JSX.Element {
  const linked = useProductProjects(product.id);
  const unlink = useUnlinkProductProject();
  const [pickerOpen, setPickerOpen] = useState(false);

  const rows = linked.data ?? [];

  return (
    <div className="flex flex-col gap-4" data-testid="product-projects">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">
          Linked projects{" "}
          <span className="text-muted-foreground">({rows.length})</span>
        </h3>
        <Button
          variant="outline"
          size="sm"
          onClick={() => setPickerOpen(true)}
          data-testid="product-project-add"
        >
          <PlusIcon className="mr-1.5 h-4 w-4" /> Add project
        </Button>
      </div>

      {linked.isError ? (
        <Alert variant="destructive" data-testid="product-projects-error">
          <AlertTitle>Couldn't load linked projects</AlertTitle>
          <AlertDescription>{linked.error.message}</AlertDescription>
        </Alert>
      ) : linked.isPending ? (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 2 }).map((_, i) => (
            <Skeleton key={i} className="h-12 rounded-md" />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <div
          className="rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-10 text-center text-sm text-muted-foreground"
          data-testid="product-projects-empty"
        >
          No projects linked yet. Link a project to tie this product to
          the work that builds it.
        </div>
      ) : (
        <ul className="flex flex-col divide-y rounded-md border">
          {rows.map((p) => (
            <li
              key={p.id}
              className="flex items-center gap-3 px-3 py-2.5 text-sm"
              data-testid="product-project-row"
            >
              <LinkIcon className="h-4 w-4 shrink-0 text-muted-foreground" />
              <button
                type="button"
                onClick={() => navigate(projectDetailRoute(p.id))}
                className="min-w-0 flex-1 truncate text-left hover:underline"
                title={p.name}
              >
                {p.name}
              </button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() =>
                  unlink.mutate({ productId: product.id, projectId: p.id })
                }
                disabled={unlink.isPending}
                title="Unlink"
                data-testid="product-project-unlink"
              >
                <XIcon className="h-4 w-4" />
              </Button>
            </li>
          ))}
        </ul>
      )}

      <ProjectPickerDialog
        open={pickerOpen}
        onOpenChange={setPickerOpen}
        orgId={product.org_id}
        linkedIds={new Set(rows.map((p) => p.id))}
        productId={product.id}
      />
    </div>
  );
}

function ProjectPickerDialog({
  open,
  onOpenChange,
  orgId,
  linkedIds,
  productId,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  orgId: string;
  linkedIds: Set<string>;
  productId: string;
}): JSX.Element {
  const [search, setSearch] = useState("");
  const link = useLinkProductProject();

  const projectsQ = useQuery({
    queryKey: ["projects", "list", { org_id: orgId, q: search.trim(), limit: 100 }],
    queryFn: () =>
      api.listProjects({
        org_id: orgId,
        q: search.trim() || undefined,
        limit: 100,
      }),
    enabled: open,
    staleTime: 30_000,
  });

  const candidates = useMemo(
    () => (projectsQ.data?.rows ?? []).filter((p) => !linkedIds.has(p.id)),
    [projectsQ.data, linkedIds],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md" data-testid="project-picker-dialog">
        <DialogHeader>
          <DialogTitle>Link a project</DialogTitle>
          <DialogDescription>
            Pick a project in the same org to link to this product.
          </DialogDescription>
        </DialogHeader>

        <Input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search projects…"
          data-testid="project-picker-search"
        />

        <div className="max-h-72 overflow-y-auto">
          {projectsQ.isPending ? (
            <div className="flex flex-col gap-2 py-2">
              {Array.from({ length: 4 }).map((_, i) => (
                <Skeleton key={i} className="h-9 rounded-md" />
              ))}
            </div>
          ) : projectsQ.isError ? (
            <Alert variant="destructive">
              <AlertDescription>{projectsQ.error.message}</AlertDescription>
            </Alert>
          ) : candidates.length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">
              No more projects to link.
            </p>
          ) : (
            <ul className="flex flex-col gap-1 py-1">
              {candidates.map((p) => (
                <li key={p.id}>
                  <button
                    type="button"
                    onClick={() =>
                      link.mutate(
                        { productId, projectId: p.id },
                        { onSuccess: () => onOpenChange(false) },
                      )
                    }
                    disabled={link.isPending}
                    className="flex w-full items-center justify-between gap-2 rounded-md border border-border px-3 py-2 text-left text-sm hover:bg-accent/30 disabled:opacity-50"
                    data-testid="project-picker-option"
                  >
                    <span className="truncate">{p.name}</span>
                    <PlusIcon className="h-4 w-4 shrink-0 text-muted-foreground" />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        {link.isError && (
          <Alert variant="destructive">
            <AlertDescription>{link.error.message}</AlertDescription>
          </Alert>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
