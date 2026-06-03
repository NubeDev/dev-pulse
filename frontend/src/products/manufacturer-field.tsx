/**
 * Manufacturer picker with an inline "New manufacturer" create flow
 * (§7.4). A product's manufacturer is a `dp_manufacturers` party; the
 * directory lives under Products ▸ Parties, but you almost always want
 * to add one *while* defining a product — so this control offers both
 * "pick an existing one" and "create one right here".
 *
 * Used by the new-product dialog and the product Overview tab. The
 * caller owns the value (`manufacturer_id | null`) and persists it
 * (create-body field or a CAS PATCH); this component only resolves the
 * id ↔ name mapping and the create modal.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { Alert, AlertDescription } from "@/components/ui/alert";
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
import { PlusIcon } from "lucide-react";

import { api } from "../api/client.js";
import type { PartyDto } from "../api/schemas/products.js";

import { useCreateParty } from "./use-products-data.js";

// radix Select can't hold an empty string value, so map "no
// manufacturer" to this sentinel and back to `null` at the boundary.
const NO_MFG = "__none__";

export interface ManufacturerFieldProps {
  /** Org the manufacturer list is scoped to. */
  orgId: string;
  /** Currently selected manufacturer id (or `null`). */
  value: string | null;
  /** Called with the new selection (`null` = None). */
  onChange: (id: string | null) => void;
  label?: string;
  disabled?: boolean;
}

export function ManufacturerField({
  orgId,
  value,
  onChange,
  label = "Manufacturer",
  disabled,
}: ManufacturerFieldProps): JSX.Element {
  const [createOpen, setCreateOpen] = useState(false);
  // Manufacturers created in this session are merged into the options
  // immediately so the freshly-picked one renders before the list
  // query refetches.
  const [justCreated, setJustCreated] = useState<PartyDto[]>([]);

  const listQ = useQuery({
    queryKey: ["parties", "manufacturers", "list", { org_id: orgId, limit: 500 }],
    queryFn: () => api.listManufacturers({ org_id: orgId, limit: 500 }),
    enabled: orgId.length > 0,
    staleTime: 60_000,
  });

  const byId = new Map<string, PartyDto>();
  for (const m of listQ.data?.rows ?? []) byId.set(m.id, m);
  for (const m of justCreated) byId.set(m.id, m);
  const options = Array.from(byId.values()).sort((a, b) =>
    a.name.localeCompare(b.name),
  );

  return (
    <div className="flex flex-col gap-1.5">
      <Label className="text-xs font-medium text-muted-foreground">
        {label}
      </Label>
      <div className="flex items-center gap-2">
        <Select
          value={value ?? NO_MFG}
          onValueChange={(v) => onChange(v === NO_MFG ? null : v)}
          disabled={disabled || orgId.length === 0}
        >
          <SelectTrigger
            className="flex-1"
            data-testid="manufacturer-field-select"
          >
            <SelectValue placeholder="None" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={NO_MFG}>None</SelectItem>
            {options.map((m) => (
              <SelectItem key={m.id} value={m.id}>
                {m.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          onClick={() => setCreateOpen(true)}
          disabled={disabled || orgId.length === 0}
          data-testid="manufacturer-field-new"
        >
          <PlusIcon className="mr-1 h-4 w-4" /> New
        </Button>
      </div>

      <NewManufacturerDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        orgId={orgId}
        onCreated={(m) => {
          setJustCreated((cur) => [...cur, m]);
          onChange(m.id);
          setCreateOpen(false);
        }}
      />
    </div>
  );
}

function NewManufacturerDialog({
  open,
  onOpenChange,
  orgId,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  orgId: string;
  onCreated: (m: PartyDto) => void;
}): JSX.Element {
  const create = useCreateParty("manufacturers");
  const [name, setName] = useState("");
  const [contactName, setContactName] = useState("");
  const [email, setEmail] = useState("");
  const [website, setWebsite] = useState("");

  const onOpen = (next: boolean): void => {
    if (next) {
      setName("");
      setContactName("");
      setEmail("");
      setWebsite("");
      create.reset();
    }
    onOpenChange(next);
  };

  const opt = (s: string): string | null => (s.trim() ? s.trim() : null);
  const canSubmit = name.trim().length > 0 && orgId.length > 0 && !create.isPending;

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!canSubmit) return;
    create.mutate(
      {
        org_id: orgId,
        name: name.trim(),
        contact_name: opt(contactName),
        email: opt(email),
        website: opt(website),
      },
      { onSuccess: (m) => onCreated(m as PartyDto) },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpen}>
      <DialogContent
        className="sm:max-w-md"
        data-testid="new-manufacturer-dialog"
      >
        <DialogHeader>
          <DialogTitle>New manufacturer</DialogTitle>
          <DialogDescription>
            Add a manufacturer to the directory. It will be selected for
            this product and available across the catalogue.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-3" onSubmit={onSubmit}>
          <div className="flex flex-col gap-2">
            <Label htmlFor="new-mfg-name">Name</Label>
            <Input
              id="new-mfg-name"
              data-testid="new-mfg-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Acme Manufacturing"
              maxLength={200}
              autoFocus
              required
            />
          </div>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="flex flex-col gap-2">
              <Label htmlFor="new-mfg-contact">Contact name</Label>
              <Input
                id="new-mfg-contact"
                value={contactName}
                onChange={(e) => setContactName(e.target.value)}
                maxLength={200}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="new-mfg-email">Email</Label>
              <Input
                id="new-mfg-email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                maxLength={200}
              />
            </div>
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="new-mfg-website">Website</Label>
            <Input
              id="new-mfg-website"
              value={website}
              onChange={(e) => setWebsite(e.target.value)}
              placeholder="https://…"
              maxLength={200}
            />
          </div>

          {create.isError && (
            <Alert variant="destructive" data-testid="new-mfg-error">
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
            <Button type="submit" disabled={!canSubmit} data-testid="new-mfg-submit">
              {create.isPending ? "Creating…" : "Create manufacturer"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
