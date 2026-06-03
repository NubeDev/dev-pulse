/**
 * `#/products` — Product & Manufacturing hub (§7.4).
 *
 * Card grid of products with a status filter chip row + a search box
 * (name / model#) and a "Create product" dialog. Loading skeletons,
 * an empty state with a CTA, and an error alert mirror the projects
 * surfaces' state handling.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { PackageIcon, PlusIcon, SearchIcon } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";

import { api } from "../api/client.js";
import type { OrgDto } from "../api/client.js";
import type { ProductDto, ProductStatus } from "../api/schemas/products.js";
import { PageHeading } from "../components/page-heading.jsx";
import {
  navigate,
  productDetailRoute,
  productsRoute,
  productsStatusOf,
  useRoute,
  type ProductStatusRoute,
} from "../routes.js";

import { NewProductDialog } from "./new-product-dialog.js";
import { useProducts } from "./use-products-data.js";

const STATUS_LABEL: Record<ProductStatus, string> = {
  draft: "Draft",
  active: "Active",
  eol: "EOL",
  archived: "Archived",
};

export const PRODUCT_STATUS_VARIANT: Record<
  ProductStatus,
  "default" | "secondary" | "outline"
> = {
  active: "default",
  draft: "secondary",
  eol: "outline",
  archived: "outline",
};

const FILTERS: Array<{ value: ProductStatusRoute | null; label: string }> = [
  { value: null, label: "All" },
  { value: "active", label: "Active" },
  { value: "draft", label: "Draft" },
  { value: "eol", label: "EOL" },
  { value: "archived", label: "Archived" },
];

export function ProductListPage(): JSX.Element {
  const route = useRoute();
  const status = productsStatusOf(route);
  const [search, setSearch] = useState("");
  const [createOpen, setCreateOpen] = useState(false);

  const query = useProducts({
    status: status ?? undefined,
    q: search.trim() || undefined,
    limit: 200,
  });

  // Resolve manufacturer names for the card subtitle without a second
  // round-trip per card.
  const manufacturers = useQuery({
    queryKey: ["parties", "manufacturers", "name-map"],
    queryFn: () => api.listManufacturers({ limit: 500 }),
    staleTime: 60_000,
  });
  const mfgName = useMemo(() => {
    const m = new Map<string, string>();
    for (const row of manufacturers.data?.rows ?? []) m.set(row.id, row.name);
    return m;
  }, [manufacturers.data]);

  const rows = query.data?.rows ?? [];

  return (
    <div className="flex flex-col gap-4 px-4 lg:px-6" data-testid="product-list">
      <PageHeading
        title="Products"
        description="Catalogue of products you build, with their manuals, documents, and linked projects."
        trailing={
          <Button
            onClick={() => setCreateOpen(true)}
            data-testid="product-create-button"
          >
            <PlusIcon className="mr-1.5 h-4 w-4" /> Create product
          </Button>
        }
      />

      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-wrap gap-1.5" data-testid="product-status-filters">
          {FILTERS.map((f) => {
            const active = (f.value ?? null) === (status ?? null);
            return (
              <a
                key={f.label}
                href={productsRoute(f.value)}
                className={cn(
                  "rounded-full border px-3 py-1 text-xs font-medium transition-colors",
                  active
                    ? "border-primary bg-primary text-primary-foreground"
                    : "border-border bg-background text-muted-foreground hover:bg-accent/40",
                )}
                data-testid={`product-status-filter-${f.value ?? "all"}`}
              >
                {f.label}
              </a>
            );
          })}
        </div>
        <div className="relative sm:w-72">
          <SearchIcon className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search name or model #…"
            className="pl-8"
            data-testid="product-search"
          />
        </div>
      </div>

      {query.isError ? (
        <Alert variant="destructive" data-testid="product-list-error">
          <AlertTitle>Couldn't load products</AlertTitle>
          <AlertDescription>{query.error.message}</AlertDescription>
        </Alert>
      ) : query.isPending ? (
        <div
          className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3"
          data-testid="product-list-loading"
        >
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-28 rounded-xl" />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <div
          className="flex flex-col items-center gap-3 rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-16 text-center"
          data-testid="product-list-empty"
        >
          <PackageIcon className="h-8 w-8 text-muted-foreground" />
          <div className="text-sm text-muted-foreground">
            {search.trim() || status
              ? "No products match the current filters."
              : "No products yet. Create your first product to track its manuals, documents, and serials."}
          </div>
          <Button onClick={() => setCreateOpen(true)}>
            <PlusIcon className="mr-1.5 h-4 w-4" /> Create product
          </Button>
        </div>
      ) : (
        <div
          className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3"
          data-testid="product-grid"
        >
          {rows.map((p) => (
            <ProductCard
              key={p.id}
              product={p}
              manufacturer={
                p.manufacturer_id ? mfgName.get(p.manufacturer_id) ?? null : null
              }
            />
          ))}
        </div>
      )}

      <NewProductDialog open={createOpen} onOpenChange={setCreateOpen} />
    </div>
  );
}

function ProductCard({
  product,
  manufacturer,
}: {
  product: ProductDto;
  manufacturer: string | null;
}): JSX.Element {
  return (
    <a
      href={productDetailRoute(product.id)}
      onClick={(e) => {
        e.preventDefault();
        navigate(productDetailRoute(product.id));
      }}
      className="block focus:outline-none"
      data-testid="product-card"
    >
      <Card className="h-full gap-2 py-4 transition-colors hover:border-primary/40 hover:bg-accent/20">
        <CardHeader className="px-4">
          <div className="flex items-start justify-between gap-2">
            <CardTitle className="min-w-0 truncate text-base" title={product.name}>
              {product.name}
            </CardTitle>
            <Badge
              variant={PRODUCT_STATUS_VARIANT[product.status]}
              data-testid="product-card-status"
            >
              {STATUS_LABEL[product.status]}
            </Badge>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-1 px-4">
          <span className="font-mono text-xs text-muted-foreground">
            {product.model_number || "—"}
          </span>
          <span className="truncate text-xs text-muted-foreground">
            {manufacturer ? manufacturer : "No manufacturer"}
          </span>
        </CardContent>
      </Card>
    </a>
  );
}

/** Shared org picker / manufacturer picker helpers reused by the
 *  create + edit dialogs. Kept here so the list page owns the small
 *  amount of shared product-form state without a third file. */
export function useOrgsForCreate(open: boolean) {
  return useQuery<OrgDto[]>({
    queryKey: ["orgs"],
    queryFn: () => api.listOrgs(),
    enabled: open,
    staleTime: 5 * 60_000,
  });
}
