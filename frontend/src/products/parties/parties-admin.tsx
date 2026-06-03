/**
 * `#/manufacturing/parties` — parties admin (§7.4).
 *
 * Three near-identical list + edit screens (customers / manufacturers
 * / suppliers) built from ONE shared `PartyList` component, switched
 * by a sub-nav driven off `?kind=…`. The Suppliers screen is flagged
 * "scaffold / not yet wired" per the P1 spec.
 *
 * Create / edit dialogs reuse the exec-summary `form-fields` controls.
 */

import { useState } from "react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
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
import { cn } from "@/lib/utils";
import { PlusIcon, SearchIcon } from "lucide-react";

import { useQuery } from "@tanstack/react-query";

import { api } from "../../api/client.js";
import type { OrgDto } from "../../api/client.js";
import type {
  CreateCustomerRequest,
  PartyDto,
  PatchCustomerRequest,
} from "../../api/schemas/products.js";
import { PageHeading } from "../../components/page-heading.jsx";
import {
  PlainTextareaField,
  TextField,
} from "../../projects/exec-summary/form-fields.js";
import {
  customerDetailRoute,
  navigate,
  partiesKindOf,
  partiesRoute,
  useRoute,
  type PartiesKindRoute,
} from "../../routes.js";
import {
  useCreateParty,
  useParties,
  usePatchParty,
  type PartyKind,
} from "../use-products-data.js";

const KIND_TABS: Array<{ value: PartiesKindRoute; label: string }> = [
  { value: "customers", label: "Customers" },
  { value: "manufacturers", label: "Manufacturers" },
  { value: "suppliers", label: "Suppliers" },
];

const KIND_NOUN: Record<PartyKind, string> = {
  customers: "customer",
  manufacturers: "manufacturer",
  suppliers: "supplier",
};

export function PartiesAdminPage(): JSX.Element {
  const route = useRoute();
  const kind = partiesKindOf(route);

  return (
    <div className="flex flex-col gap-4 px-4 lg:px-6" data-testid="parties-admin">
      <PageHeading
        title="Parties"
        description="Customers, manufacturers, and suppliers used across products and manufacturing."
      />

      <div className="flex flex-wrap gap-1.5" data-testid="parties-kind-tabs">
        {KIND_TABS.map((t) => {
          const active = t.value === kind;
          return (
            <a
              key={t.value}
              href={partiesRoute(t.value)}
              className={cn(
                "rounded-full border px-3 py-1 text-xs font-medium transition-colors",
                active
                  ? "border-primary bg-primary text-primary-foreground"
                  : "border-border bg-background text-muted-foreground hover:bg-accent/40",
              )}
              data-testid={`parties-kind-${t.value}`}
            >
              {t.label}
            </a>
          );
        })}
      </div>

      <PartyList kind={kind} />
    </div>
  );
}

function PartyList({ kind }: { kind: PartyKind }): JSX.Element {
  const [search, setSearch] = useState("");
  const [createOpen, setCreateOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<PartyDto | null>(null);

  const list = useParties(kind, { q: search.trim() || undefined, limit: 200 });
  const rows = list.data?.rows ?? [];
  const isSupplier = kind === "suppliers";

  return (
    <div className="flex flex-col gap-3" data-testid={`party-list-${kind}`}>
      {isSupplier && (
        <Alert data-testid="suppliers-scaffold-note">
          <AlertTitle>Scaffold — not yet wired</AlertTitle>
          <AlertDescription>
            The Suppliers surface is a P1 scaffold. The list + edit
            flow runs against the API, but supplier relationships to
            products / BOMs land in a later phase.
          </AlertDescription>
        </Alert>
      )}

      <div className="flex items-center justify-between gap-3">
        <div className="relative sm:w-72">
          <SearchIcon className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={`Search ${KIND_NOUN[kind]}s…`}
            className="pl-8"
            data-testid="party-search"
          />
        </div>
        <Button onClick={() => setCreateOpen(true)} data-testid="party-create-button">
          <PlusIcon className="mr-1.5 h-4 w-4" /> New {KIND_NOUN[kind]}
        </Button>
      </div>

      {list.isError ? (
        <Alert variant="destructive" data-testid="party-list-error">
          <AlertTitle>Couldn't load {KIND_NOUN[kind]}s</AlertTitle>
          <AlertDescription>{list.error.message}</AlertDescription>
        </Alert>
      ) : list.isPending ? (
        <div className="flex flex-col gap-2" data-testid="party-list-loading">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={i} className="h-14 rounded-md" />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <div
          className="flex flex-col items-center gap-3 rounded-lg border-2 border-dashed border-border bg-muted/20 px-4 py-14 text-center"
          data-testid="party-list-empty"
        >
          <div className="text-sm text-muted-foreground">
            No {KIND_NOUN[kind]}s yet.
          </div>
          <Button size="sm" onClick={() => setCreateOpen(true)}>
            <PlusIcon className="mr-1.5 h-4 w-4" /> New {KIND_NOUN[kind]}
          </Button>
        </div>
      ) : (
        <ul className="flex flex-col divide-y rounded-md border">
          {rows.map((p) => (
            <li
              key={p.id}
              className="flex items-center gap-3 px-3 py-3 text-sm"
              data-testid="party-row"
            >
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="flex items-center gap-2 truncate font-medium">
                  {p.name}
                  {p.archived_at ? (
                    <Badge variant="outline" className="text-[10px]">
                      Archived
                    </Badge>
                  ) : null}
                </span>
                <span className="flex flex-wrap gap-x-3 text-xs text-muted-foreground">
                  {p.contact_name ? <span>{p.contact_name}</span> : null}
                  {p.email ? <span>{p.email}</span> : null}
                  {p.phone ? <span>{p.phone}</span> : null}
                </span>
              </div>
              {kind === "customers" && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => navigate(customerDetailRoute(p.id))}
                  data-testid="party-view"
                >
                  View
                </Button>
              )}
              <Button
                variant="outline"
                size="sm"
                onClick={() => setEditTarget(p)}
                data-testid="party-edit"
              >
                Edit
              </Button>
            </li>
          ))}
        </ul>
      )}

      <PartyDialog
        kind={kind}
        open={createOpen}
        onOpenChange={setCreateOpen}
        party={null}
      />
      <PartyDialog
        kind={kind}
        open={editTarget !== null}
        onOpenChange={(open) => {
          if (!open) setEditTarget(null);
        }}
        party={editTarget}
      />
    </div>
  );
}

interface PartyForm {
  name: string;
  contact_name: string;
  email: string;
  phone: string;
  address: string;
  website: string;
  notes: string;
  account_ref: string;
}

function emptyForm(party: PartyDto | null): PartyForm {
  const c = party as (PartyDto & { account_ref?: string | null }) | null;
  return {
    name: party?.name ?? "",
    contact_name: party?.contact_name ?? "",
    email: party?.email ?? "",
    phone: party?.phone ?? "",
    address: party?.address ?? "",
    website: party?.website ?? "",
    notes: party?.notes ?? "",
    account_ref: c?.account_ref ?? "",
  };
}

function PartyDialog({
  kind,
  open,
  onOpenChange,
  party,
}: {
  kind: PartyKind;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  party: PartyDto | null;
}): JSX.Element {
  const isEdit = party !== null;
  const create = useCreateParty(kind);
  const patch = usePatchParty(kind);
  const [form, setForm] = useState<PartyForm>(() => emptyForm(party));
  const [orgId, setOrgId] = useState("");

  const orgsQ = useQuery<OrgDto[]>({
    queryKey: ["orgs"],
    queryFn: () => api.listOrgs(),
    enabled: open && !isEdit,
    staleTime: 5 * 60_000,
  });

  // Re-seed on open via the open-change wrapper.
  const onOpen = (next: boolean): void => {
    if (next) {
      setForm(emptyForm(party));
      setOrgId(orgsQ.data?.[0]?.id ?? "");
      create.reset();
      patch.reset();
    }
    onOpenChange(next);
  };

  const set = (k: keyof PartyForm) => (v: string | null) =>
    setForm((f) => ({ ...f, [k]: v ?? "" }));

  const busy = create.isPending || patch.isPending;
  const error = create.error ?? patch.error;
  const nameError = form.name.trim().length === 0;
  // For create we need an org id; once orgs load, default to the first.
  const effectiveOrgId = orgId || orgsQ.data?.[0]?.id || "";
  const canSubmit = !nameError && !busy && (isEdit || effectiveOrgId.length > 0);

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!canSubmit) return;
    const opt = (s: string): string | null => (s.trim() ? s.trim() : null);
    if (isEdit && party) {
      const body: PatchCustomerRequest = {
        expected_version: party.version,
        name: form.name.trim(),
        contact_name: opt(form.contact_name),
        email: opt(form.email),
        phone: opt(form.phone),
        address: opt(form.address),
        website: opt(form.website),
        notes: opt(form.notes),
      };
      if (kind === "customers") body.account_ref = opt(form.account_ref);
      patch.mutate(
        { id: party.id, body },
        { onSuccess: () => onOpenChange(false) },
      );
    } else {
      const body: CreateCustomerRequest = {
        org_id: effectiveOrgId,
        name: form.name.trim(),
        contact_name: opt(form.contact_name),
        email: opt(form.email),
        phone: opt(form.phone),
        address: opt(form.address),
        website: opt(form.website),
        notes: opt(form.notes),
      };
      if (kind === "customers") body.account_ref = opt(form.account_ref);
      create.mutate(body, { onSuccess: () => onOpenChange(false) });
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpen}>
      <DialogContent className="sm:max-w-lg" data-testid="party-dialog">
        <DialogHeader>
          <DialogTitle>
            {isEdit ? `Edit ${KIND_NOUN[kind]}` : `New ${KIND_NOUN[kind]}`}
          </DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Update contact details. Saves under CAS."
              : `Add a ${KIND_NOUN[kind]} to the directory.`}
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-3" onSubmit={onSubmit}>
          <TextField
            label="Name"
            value={form.name}
            onCommit={set("name")}
          />
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <TextField
              label="Contact name"
              value={form.contact_name}
              onCommit={set("contact_name")}
            />
            <TextField label="Email" value={form.email} onCommit={set("email")} />
            <TextField label="Phone" value={form.phone} onCommit={set("phone")} />
            <TextField
              label="Website"
              value={form.website}
              onCommit={set("website")}
            />
          </div>
          {kind === "customers" && (
            <TextField
              label="Account ref"
              value={form.account_ref}
              onCommit={set("account_ref")}
              hint="Your internal account / customer reference."
            />
          )}
          <PlainTextareaField
            label="Address"
            value={form.address}
            onCommit={set("address")}
            rows={2}
          />
          <PlainTextareaField
            label="Notes"
            value={form.notes}
            onCommit={set("notes")}
            rows={2}
          />

          {error && (
            <Alert variant="destructive" data-testid="party-dialog-error">
              <AlertDescription>{error.message}</AlertDescription>
            </Alert>
          )}

          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={busy}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={!canSubmit} data-testid="party-submit">
              {busy ? "Saving…" : isEdit ? "Save" : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
