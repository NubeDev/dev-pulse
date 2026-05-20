/**
 * Pins manager — SCOPE-PROJECTS §6 admin surface.
 *
 * The sidebar widget (`PinSidebar`) gives the always-visible
 * cross-page shortcut; this page is where the operator does the
 * curation work: see every pin, drop ones they no longer need, and
 * (in a follow-up stage) reorder them by drag. Reorder uses the
 * §6.4 `PUT /me/pins/order` atomic-set endpoint, so partial state
 * is not observable from anywhere — see `useReorderPins`.
 *
 * The "add pin" affordance is intentionally minimal here: the
 * sidebar pickers in the report views (a follow-up) are the
 * primary entry point. This page is for *managing* an existing
 * collection.
 */

import { IconArrowDown, IconArrowUp, IconPinnedOff } from "@tabler/icons-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

import { PIN_CAP, PIN_RENDER_CAP, type PinDto } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";
import { usePins, useRemovePin, useReorderPins } from "./use-workflow-data.js";

export function PinsPage(): JSX.Element {
  const pins = usePins();
  const remove = useRemovePin();
  const reorder = useReorderPins();

  const data = pins.data ?? [];

  const move = (from: number, to: number): void => {
    if (to < 0 || to >= data.length) return;
    const next = [...data];
    const [row] = next.splice(from, 1);
    if (!row) return;
    next.splice(to, 0, row);
    reorder.mutate({
      order: next.map((p) => ({ kind: p.kind, target_id: p.target_id })),
    });
  };

  return (
    <div className="flex flex-col gap-6 px-4 lg:px-6" data-testid="pins-page">
      <PageHeading
        title="Pins"
        description="Per-user, ordered. Pin a tag to follow every repo it spans without re-pinning by hand (§6.1). Pinning a tag counts as one pin against the data-model cap; the rendered sidebar collapses above the render cap into an overflow disclosure."
        trailing={
          <Badge variant="outline" data-testid="pin-cap-indicator">
            {data.length} / {PIN_CAP} pinned
          </Badge>
        }
      />
      {pins.isLoading ? (
        <Alert><AlertDescription>Loading pins…</AlertDescription></Alert>
      ) : pins.isError ? (
        <Alert variant="destructive">
          <AlertTitle>Could not load pins</AlertTitle>
          <AlertDescription>
            {pins.error instanceof Error ? pins.error.message : "Unknown"}
          </AlertDescription>
        </Alert>
      ) : data.length === 0 ? (
        <Card>
          <CardContent className="py-6 text-sm text-muted-foreground">
            No pins yet. Pin a repo or a tag from a report row to populate
            the sidebar.
          </CardContent>
        </Card>
      ) : (
        <Card>
          <CardHeader>
            <CardTitle>Pinned items</CardTitle>
          </CardHeader>
          <CardContent>
            <ol className="flex flex-col gap-2" data-testid="pins-list">
              {data.map((p, i) => (
                <PinRow
                  key={`${p.kind}:${p.target_id}`}
                  pin={p}
                  index={i}
                  last={i === data.length - 1}
                  onUp={() => move(i, i - 1)}
                  onDown={() => move(i, i + 1)}
                  onRemove={() =>
                    remove.mutate({ kind: p.kind, target_id: p.target_id })
                  }
                />
              ))}
            </ol>
            <p className="pt-4 text-xs text-muted-foreground">
              Sidebar render cap: {PIN_RENDER_CAP} entries after tag expansion
              — pinning a tag spanning many repos can fill the sidebar on its
              own, after which the rest collapses into "…and N more" (§13.5).
            </p>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function PinRow({
  pin,
  index,
  last,
  onUp,
  onDown,
  onRemove,
}: {
  pin: PinDto;
  index: number;
  last: boolean;
  onUp: () => void;
  onDown: () => void;
  onRemove: () => void;
}): JSX.Element {
  return (
    <li
      className="flex items-center gap-2 rounded border border-border/50 px-3 py-2"
      data-testid={`pin-row-${index}`}
    >
      <span className="w-6 text-xs text-muted-foreground">{index + 1}</span>
      <Badge variant="outline" className="capitalize">{pin.kind}</Badge>
      <code className="truncate text-xs">{pin.target_id}</code>
      <div className="ml-auto flex gap-1">
        <Button
          size="sm"
          variant="ghost"
          onClick={onUp}
          disabled={index === 0}
          aria-label="Move up"
        >
          <IconArrowUp className="size-4" />
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={onDown}
          disabled={last}
          aria-label="Move down"
        >
          <IconArrowDown className="size-4" />
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={onRemove}
          aria-label="Remove pin"
        >
          <IconPinnedOff className="size-4" />
        </Button>
      </div>
    </li>
  );
}
