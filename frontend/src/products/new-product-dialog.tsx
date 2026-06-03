/**
 * "Create product" modal (§7.4). Name, model #, org, manufacturer,
 * and status. `org_id` is sourced the same way the §6.2 new-project
 * modal does it — `GET /orgs` with the first row as the default — so
 * the two create flows share one mechanism.
 *
 * On success the parent invalidates the products cache and the host
 * navigates to the new product's detail route.
 */

import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";

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
import { Spinner } from "@/components/ui/spinner";

import { api } from "../api/client.js";
import type { OrgDto } from "../api/client.js";
import type { ProductDto, ProductStatus } from "../api/schemas/products.js";
import { navigate, productDetailRoute } from "../routes.js";

import { useCreateProduct } from "./use-products-data.js";

const STATUSES: ProductStatus[] = ["draft", "active", "eol", "archived"];
const STATUS_LABEL: Record<ProductStatus, string> = {
  draft: "Draft",
  active: "Active",
  eol: "EOL",
  archived: "Archived",
};

// Sentinel for "no manufacturer" — radix Select can't hold an empty
// string value, so we map this to `null` on submit.
const NO_MFG = "__none__";

export interface NewProductDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Pre-select an org id; falls back to the first visible org. */
  defaultOrgId?: string | null;
  /** Called after a successful create (defaults to navigating to the
   *  new product's detail page). */
  onCreated?: (product: ProductDto) => void;
}

export function NewProductDialog({
  open,
  onOpenChange,
  defaultOrgId,
  onCreated,
}: NewProductDialogProps): JSX.Element {
  const orgsQ = useQuery<OrgDto[]>({
    queryKey: ["orgs"],
    queryFn: () => api.listOrgs(),
    enabled: open,
    staleTime: 5 * 60_000,
  });
  const create = useCreateProduct();

  const [name, setName] = useState("");
  const [modelNumber, setModelNumber] = useState("");
  const [orgId, setOrgId] = useState("");
  const [manufacturerId, setManufacturerId] = useState<string>(NO_MFG);
  const [status, setStatus] = useState<ProductStatus>("draft");

  // Manufacturer options scoped to the chosen org.
  const manufacturersQ = useQuery({
    queryKey: ["parties", "manufacturers", "list", { org_id: orgId, limit: 500 }],
    queryFn: () => api.listManufacturers({ org_id: orgId, limit: 500 }),
    enabled: open && orgId.length > 0,
    staleTime: 60_000,
  });

  useEffect(() => {
    if (!open) return;
    setName("");
    setModelNumber("");
    setManufacturerId(NO_MFG);
    setStatus("draft");
    create.reset();
    setOrgId(defaultOrgId ?? orgsQ.data?.[0]?.id ?? "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, defaultOrgId]);

  useEffect(() => {
    if (open && !orgId && orgsQ.data && orgsQ.data.length > 0) {
      setOrgId(orgsQ.data[0]!.id);
    }
  }, [open, orgId, orgsQ.data]);

  // Reset the manufacturer when switching orgs — the picked party is
  // scoped to the previous org.
  useEffect(() => {
    setManufacturerId(NO_MFG);
  }, [orgId]);

  const orgs = orgsQ.data ?? [];
  const canSubmit =
    name.trim().length > 0 &&
    modelNumber.trim().length > 0 &&
    orgId.length > 0 &&
    !create.isPending;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!canSubmit) return;
    create.mutate(
      {
        org_id: orgId,
        name: name.trim(),
        model_number: modelNumber.trim(),
        manufacturer_id: manufacturerId === NO_MFG ? null : manufacturerId,
        status,
      },
      {
        onSuccess: (product) => {
          if (onCreated) onCreated(product);
          else navigate(productDetailRoute(product.id));
          onOpenChange(false);
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg" data-testid="new-product-dialog">
        <DialogHeader>
          <DialogTitle>New product</DialogTitle>
          <DialogDescription>
            A product groups its manuals, documents, serial-format
            config, and linked projects. You can edit the description
            and serial format after it's created.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
          <div className="flex flex-col gap-2">
            <Label htmlFor="new-product-name">Name</Label>
            <Input
              id="new-product-name"
              data-testid="new-product-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Rubix Controller"
              maxLength={200}
              autoFocus
              required
            />
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="new-product-model">Model #</Label>
            <Input
              id="new-product-model"
              data-testid="new-product-model"
              value={modelNumber}
              onChange={(e) => setModelNumber(e.target.value)}
              placeholder="RX-200"
              maxLength={200}
              required
            />
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="new-product-org">Org</Label>
            {orgsQ.isPending ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Spinner /> Loading orgs…
              </div>
            ) : (
              <Select value={orgId} onValueChange={setOrgId}>
                <SelectTrigger id="new-product-org" data-testid="new-product-org">
                  <SelectValue placeholder="Select an org" />
                </SelectTrigger>
                <SelectContent>
                  {orgs.map((o) => (
                    <SelectItem key={o.id} value={o.id}>
                      {o.name ? `${o.name} (${o.login})` : o.login}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="new-product-manufacturer">Manufacturer</Label>
            <Select value={manufacturerId} onValueChange={setManufacturerId}>
              <SelectTrigger
                id="new-product-manufacturer"
                data-testid="new-product-manufacturer"
              >
                <SelectValue placeholder="None" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_MFG}>None</SelectItem>
                {(manufacturersQ.data?.rows ?? []).map((m) => (
                  <SelectItem key={m.id} value={m.id}>
                    {m.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="new-product-status">Status</Label>
            <Select
              value={status}
              onValueChange={(v) => setStatus(v as ProductStatus)}
            >
              <SelectTrigger
                id="new-product-status"
                data-testid="new-product-status"
              >
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

          {create.isError && (
            <Alert variant="destructive" data-testid="new-product-error">
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
              data-testid="new-product-submit"
              disabled={!canSubmit}
            >
              {create.isPending ? "Creating…" : "Create product"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
