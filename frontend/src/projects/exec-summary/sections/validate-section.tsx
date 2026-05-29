/**
 * "Validate" tab — one page that lists every incomplete field across
 * every section, and lets the user fix them all without bouncing
 * between tabs.
 *
 * Field rules come from `computeMissingFields` in `validation.ts`,
 * which mirrors the backend SQL one-for-one. Short scalar fields
 * (text + number) are editable inline via the same form-field
 * controls + autosave hook the section tabs use. Long-form
 * (markdown / arrays / file uploads) get a "Open <section>" button
 * that switches to that tab and scrolls the matching input into
 * view (the section pages tag their inputs with
 * `data-validation-key`).
 */

import { useMemo } from "react";
import { CheckCircle2Icon, ChevronRightIcon } from "lucide-react";

import { Button } from "@/components/ui/button";

import type {
  ExecSummaryDto,
  ExecSummarySectionId,
} from "../../../api/client.js";
import { NumberField, TextField } from "../form-fields.js";
import { useExecSummaryAutosave } from "../hooks/use-exec-summary.js";
import {
  computeMissingFields,
  groupMissingBySection,
  type MissingField,
} from "../validation.js";

interface ValidateSectionProps {
  projectId: string;
  data: ExecSummaryDto;
  onJumpTo: (sectionId: ExecSummarySectionId, fieldKey: string) => void;
}

export function ValidateSection({
  projectId,
  data,
  onJumpTo,
}: ValidateSectionProps): JSX.Element {
  const { patch } = useExecSummaryAutosave(projectId);
  const groups = useMemo(
    () => groupMissingBySection(computeMissingFields(data)),
    [data],
  );
  const total = groups.reduce((acc, g) => acc + g.fields.length, 0);

  if (total === 0) {
    return (
      <div
        className="flex flex-col items-center gap-2 py-12 text-center"
        data-testid="exec-summary-validate-empty"
      >
        <CheckCircle2Icon className="h-10 w-10 text-emerald-500" />
        <p className="text-sm font-medium">Everything's filled in.</p>
        <p className="text-xs text-muted-foreground">
          No required fields are missing. You're clear to submit.
        </p>
      </div>
    );
  }

  return (
    <div
      className="flex flex-col gap-6"
      data-testid="exec-summary-validate"
    >
      <div className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-900">
        <span className="font-semibold">{total}</span> required{" "}
        {total === 1 ? "field is" : "fields are"} still incomplete. Fix
        them here or jump to the relevant section.
      </div>

      {groups.map((g) => (
        <section
          key={g.sectionId}
          className="flex flex-col gap-3"
          data-testid={`exec-summary-validate-group-${g.sectionId}`}
        >
          <header className="flex items-center justify-between border-b pb-2">
            <h3 className="text-sm font-semibold">{g.label}</h3>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => onJumpTo(g.sectionId, g.fields[0]!.key)}
            >
              Open section
              <ChevronRightIcon className="ml-1 h-3.5 w-3.5" />
            </Button>
          </header>
          <ul className="flex flex-col gap-3">
            {g.fields.map((f) => (
              <li key={f.key}>
                <FieldRow
                  field={f}
                  data={data}
                  patch={patch}
                  onJumpTo={onJumpTo}
                />
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

interface FieldRowProps {
  field: MissingField;
  data: ExecSummaryDto;
  patch: ReturnType<typeof useExecSummaryAutosave>["patch"];
  onJumpTo: (sectionId: ExecSummarySectionId, fieldKey: string) => void;
}

function FieldRow({
  field,
  data,
  patch,
  onJumpTo,
}: FieldRowProps): JSX.Element {
  if (field.kind === "text") {
    return (
      <InlineTextRow field={field} data={data} patch={patch} />
    );
  }
  if (field.kind === "number") {
    return (
      <InlineNumberRow field={field} data={data} patch={patch} />
    );
  }
  return <JumpRow field={field} onJumpTo={onJumpTo} />;
}

function InlineTextRow({
  field,
  data,
  patch,
}: {
  field: MissingField;
  data: ExecSummaryDto;
  patch: ReturnType<typeof useExecSummaryAutosave>["patch"];
}): JSX.Element {
  // Only `summary.product_name` is currently text-kind. Keeping the
  // dispatch generic so adding another short-text required field is
  // a one-line change in validation.ts.
  const value =
    field.key === "summary.product_name"
      ? data.summary.product_name
      : null;
  return (
    <div className="rounded-md border bg-card px-3 py-2">
      <TextField
        id={`validate-${field.key}`}
        label={field.label}
        value={value}
        hint={field.hint}
        onCommit={(next) => {
          if (field.key === "summary.product_name") {
            patch({ summary: { product_name: next } });
          }
        }}
      />
    </div>
  );
}

function InlineNumberRow({
  field,
  data,
  patch,
}: {
  field: MissingField;
  data: ExecSummaryDto;
  patch: ReturnType<typeof useExecSummaryAutosave>["patch"];
}): JSX.Element {
  if (field.key === "commercial.rrp_cents") {
    return (
      <div className="rounded-md border bg-card px-3 py-2">
        <NumberField
          id={`validate-${field.key}`}
          label={field.label}
          value={data.commercial.rrp_cents}
          onCommit={(rrp_cents) => patch({ commercial: { rrp_cents } })}
          scale={100}
          step={0.01}
          prefix="$"
          placeholder="0.00"
          hint={field.hint}
        />
      </div>
    );
  }
  if (field.key === "commercial.target_gp_pct") {
    return (
      <div className="rounded-md border bg-card px-3 py-2">
        <NumberField
          id={`validate-${field.key}`}
          label={field.label}
          value={data.commercial.target_gp_pct}
          onCommit={(target_gp_pct) =>
            patch({ commercial: { target_gp_pct } })
          }
          step={0.1}
          suffix="%"
          placeholder="0"
          hint={field.hint}
        />
      </div>
    );
  }
  return <JumpRow field={field} onJumpTo={() => {}} />;
}

function JumpRow({
  field,
  onJumpTo,
}: {
  field: MissingField;
  onJumpTo: (sectionId: ExecSummarySectionId, fieldKey: string) => void;
}): JSX.Element {
  return (
    <div className="flex items-start justify-between gap-3 rounded-md border bg-card px-3 py-2">
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium">{field.label}</p>
        {field.hint && (
          <p className="text-xs text-muted-foreground">{field.hint}</p>
        )}
      </div>
      <Button
        type="button"
        size="sm"
        variant="outline"
        onClick={() => onJumpTo(field.sectionId, field.key)}
      >
        Fix
        <ChevronRightIcon className="ml-1 h-3.5 w-3.5" />
      </Button>
    </div>
  );
}
