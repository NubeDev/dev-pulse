/**
 * `<NewViewWizard>` — two-step "New view" dialog.
 *
 *   Step 1 · Template — tile grid of [`VIEW_TEMPLATES`].
 *   Step 2 · Details — name (single/custom) or batch note, plus
 *                      dates.
 *
 * Categories are NOT part of the create flow — the user manages
 * them post-creation via the gear icon in the workbench toolbar
 * (`<CategoriesManagerDialog>`). Keeping the create form small
 * means the same wizard works for every template without an
 * "optional fork" the user has to mentally skip.
 *
 * Edit mode stays single-page — see `<EditViewDialog>`.
 */

import { useEffect, useState } from "react";
import {
  ArrowLeftIcon,
  ArrowRightIcon,
  CheckIcon,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { DateInput } from "@/components/ui/date-input";
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

import { iconForName } from "../icon-for-name.js";

import type { ProjectViewWriteBody, TagDto } from "../../api/client.js";

import {
  VIEW_TEMPLATES,
  type ViewTemplate,
} from "./templates.js";
import { DateDisplayPicker } from "./date-display-picker.js";
import { weekOfMonthLabel, type DateDisplayMode } from "./date-display.js";

export interface NewViewWizardProps {
  open: boolean;
  /** Org of the project being edited. Unused by the wizard itself
   *  now that categories moved to the post-creation manager, but
   *  kept on the props for callsite stability. */
  orgId: string;
  /** Cached tag list. Unused since categories left the create
   *  flow; kept on the props for callsite stability. */
  existingTags: readonly TagDto[] | null;
  /** Active toolbar shape — used for the `Custom` template and
   *  as the sort default for every other template. */
  current: {
    groupBy: string | null;
    filterClauses: ProjectViewWriteBody["filter_clauses"];
    sort: string;
  };
  busy?: boolean;
  onCancel: () => void;
  /** Called once for the `single` and `custom` templates.
   *
   *  `dateDisplay` is the machine-local tab badge preference — it
   *  isn't part of the write body, so the parent persists it
   *  against each freshly-created view id once the POST resolves. */
  onSubmit: (
    body: ProjectViewWriteBody,
    dateDisplay: DateDisplayMode,
  ) => void;
  /** Called once for the `batch` template, with every seed at once.
   *
   *  The wizard used to loop `onSubmit` per seed, which made each
   *  POST an independent fire-and-forget mutation — a single 409 or
   *  dropped request silently left a half-built gate strip. Handing
   *  the whole set over lets the parent create-and-verify them as
   *  one unit. */
  onSubmitBatch: (
    bodies: ProjectViewWriteBody[],
    dateDisplay: DateDisplayMode,
  ) => void;
}

type Step = "template" | "details";

export function NewViewWizard({
  open,
  current,
  busy,
  onCancel,
  onSubmit,
  onSubmitBatch,
}: NewViewWizardProps): JSX.Element {
  const [step, setStep] = useState<Step>("template");
  const [template, setTemplate] = useState<ViewTemplate | null>(null);
  const [name, setName] = useState("");
  const [startDate, setStartDate] = useState("");
  const [dueDate, setDueDate] = useState("");
  const [dateDisplay, setDateDisplay] = useState<DateDisplayMode>("week");

  // Reset every time the dialog re-opens so stale state from a
  // previous run doesn't leak.
  useEffect(() => {
    if (!open) return;
    setStep("template");
    setTemplate(null);
    setName("");
    setStartDate("");
    setDueDate("");
    setDateDisplay("week");
  }, [open]);

  const pickTemplate = (t: ViewTemplate): void => {
    setTemplate(t);
    if (t.kind === "single" && t.seed) {
      setName(t.seed.name);
    } else {
      setName("");
    }
  };

  const goNext = (): void => {
    if (!template) return;
    setStep("details");
  };

  const goBack = (): void => {
    setStep("template");
  };

  const trimmed = name.trim();

  const canSubmitDetails = ((): boolean => {
    if (!template || busy) return false;
    switch (template.kind) {
      case "batch":
        return (template.batch?.length ?? 0) > 0;
      case "single":
      case "custom":
        return trimmed.length > 0 && trimmed.length <= 60;
    }
  })();

  const submit = (): void => {
    if (!template || !canSubmitDetails) return;

    if (template.kind === "batch" && template.batch) {
      onSubmitBatch(
        template.batch.map((seed) => ({
          name: seed.name,
          group_by: seed.groupBy,
          filter_clauses: seed.filterClauses,
          sort: current.sort,
          start_date: startDate || null,
          due_date: dueDate || null,
        })),
        dateDisplay,
      );
      onCancel();
      return;
    }

    if (template.kind === "single" && template.seed) {
      onSubmit(
        {
          name: trimmed || template.seed.name,
          group_by: template.seed.groupBy,
          filter_clauses: template.seed.filterClauses,
          sort: current.sort,
          start_date: startDate || null,
          due_date: dueDate || null,
        },
        dateDisplay,
      );
      onCancel();
      return;
    }

    // custom
    onSubmit(
      {
        name: trimmed,
        group_by: current.groupBy,
        filter_clauses: current.filterClauses,
        sort: current.sort,
        start_date: startDate || null,
        due_date: dueDate || null,
      },
      dateDisplay,
    );
    onCancel();
  };

  return (
    <Dialog open={open} onOpenChange={(o) => (o ? null : onCancel())}>
      <DialogContent
        className="sm:max-w-2xl"
        data-testid="project-view-wizard"
      >
        <DialogHeader>
          <DialogTitle>New view</DialogTitle>
          <DialogDescription>
            {step === "template"
              ? "Pick a template. You can add collapsible category sections after the view is created via the gear icon in the toolbar."
              : template?.kind === "batch"
                ? "Optional shared dates for every tab in this batch."
                : "Name the view and set an optional timeline."}
          </DialogDescription>
        </DialogHeader>

        <StepIndicator step={step} />

        {step === "template" && (
          <TemplateStep selected={template} onPick={pickTemplate} />
        )}

        {step === "details" && template && (
          <DetailsStep
            template={template}
            name={name}
            onChangeName={setName}
            startDate={startDate}
            onChangeStartDate={setStartDate}
            dueDate={dueDate}
            onChangeDueDate={setDueDate}
            dateDisplay={dateDisplay}
            onChangeDateDisplay={setDateDisplay}
          />
        )}

        <DialogFooter className="sm:justify-between">
          <Button
            variant="ghost"
            onClick={step === "template" ? onCancel : goBack}
            data-testid="project-view-wizard-back"
          >
            {step === "template" ? (
              "Cancel"
            ) : (
              <>
                <ArrowLeftIcon className="mr-1 size-4" /> Back
              </>
            )}
          </Button>
          <div className="flex items-center gap-2">
            {step === "template" ? (
              <Button
                onClick={goNext}
                disabled={!template}
                data-testid="project-view-wizard-next"
              >
                Next <ArrowRightIcon className="ml-1 size-4" />
              </Button>
            ) : (
              <Button
                onClick={submit}
                disabled={!canSubmitDetails || !template}
                data-testid="project-view-wizard-submit"
              >
                {template ? submitButtonLabel(template) : "Create"}
              </Button>
            )}
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function submitButtonLabel(template: ViewTemplate): string {
  if (template.kind === "batch") {
    return `Create ${template.batch?.length ?? 0} tabs`;
  }
  return "Create view";
}

// ---------------------------------------------------------------------------
// Step indicator (two-dot bar)
// ---------------------------------------------------------------------------

function StepIndicator({ step }: { step: Step }): JSX.Element {
  const items: Array<{ id: Step; label: string }> = [
    { id: "template", label: "Template" },
    { id: "details", label: "Details" },
  ];
  return (
    <ol
      className="flex items-center gap-2 text-xs text-muted-foreground"
      data-testid="project-view-wizard-steps"
    >
      {items.map((it, i) => {
        const active = it.id === step;
        const done = i < items.findIndex((x) => x.id === step);
        return (
          <li key={it.id} className="flex items-center gap-2">
            <span
              className={[
                "flex size-5 items-center justify-center rounded-full border text-[10px] font-semibold",
                active
                  ? "border-primary bg-primary text-primary-foreground"
                  : done
                    ? "border-emerald-500 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400"
                    : "border-border text-muted-foreground",
              ].join(" ")}
            >
              {done ? <CheckIcon className="size-3" /> : i + 1}
            </span>
            <span className={active ? "font-medium text-foreground" : ""}>
              {it.label}
            </span>
            {i < items.length - 1 && (
              <span className="h-px w-6 bg-border" aria-hidden />
            )}
          </li>
        );
      })}
    </ol>
  );
}

// ---------------------------------------------------------------------------
// Step 1 — template grid
// ---------------------------------------------------------------------------

function TemplateStep({
  selected,
  onPick,
}: {
  selected: ViewTemplate | null;
  onPick: (t: ViewTemplate) => void;
}): JSX.Element {
  return (
    <div
      className="grid grid-cols-2 gap-2"
      data-testid="project-view-wizard-templates"
    >
      {VIEW_TEMPLATES.map((t) => {
        const isSelected = selected?.id === t.id;
        return (
          <button
            key={t.id}
            type="button"
            onClick={() => onPick(t)}
            data-testid={`project-view-template-${t.id}`}
            data-selected={isSelected ? "true" : "false"}
            className={
              isSelected
                ? "flex items-start gap-2 rounded-md border border-primary bg-primary/5 p-2 text-left ring-1 ring-primary"
                : "flex items-start gap-2 rounded-md border border-border p-2 text-left hover:bg-muted/40"
            }
          >
            <t.Icon className="mt-0.5 size-5 shrink-0 text-muted-foreground" />
            <div className="flex flex-col gap-0.5">
              <span className="text-sm font-medium">{t.label}</span>
              <span className="text-xs text-muted-foreground">
                {t.description}
              </span>
            </div>
          </button>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Step 2 — details + optional categories
// ---------------------------------------------------------------------------

interface DetailsStepProps {
  template: ViewTemplate;
  name: string;
  onChangeName: (v: string) => void;
  startDate: string;
  onChangeStartDate: (v: string) => void;
  dueDate: string;
  onChangeDueDate: (v: string) => void;
  dateDisplay: DateDisplayMode;
  onChangeDateDisplay: (mode: DateDisplayMode) => void;
}

function DetailsStep({
  template,
  name,
  onChangeName,
  startDate,
  onChangeStartDate,
  dueDate,
  onChangeDueDate,
  dateDisplay,
  onChangeDateDisplay,
}: DetailsStepProps): JSX.Element {
  const isBatch = template.kind === "batch";
  const PreviewIcon = iconForName(name.trim() || "view");

  return (
    <div className="flex flex-col gap-4 py-2">
      {!isBatch && (
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="project-view-name">Name</Label>
          <div className="flex items-center gap-2">
            <div
              className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border bg-muted/40"
              title="Auto-picked from the name"
            >
              <PreviewIcon className="size-4 text-muted-foreground" />
            </div>
            <Input
              id="project-view-name"
              autoFocus
              value={name}
              onChange={(e) => onChangeName(e.target.value)}
              placeholder="View name…"
              maxLength={60}
              data-testid="project-view-name-input"
            />
          </div>
          <p className="text-xs text-muted-foreground">
            Tip: words like <code>bug</code>, <code>gate</code>,{" "}
            <code>blocked</code> auto-pick a matching icon.
          </p>
        </div>
      )}

      {isBatch && (
        <p className="text-xs text-muted-foreground">
          <strong>{template.batch?.length ?? 0}</strong> tabs will be
          created in one click. Each gets the shared dates; the tab
          icons are auto-picked from their names. Add category
          sections after creation via the gear icon in the toolbar.
        </p>
      )}

      <div className="grid grid-cols-2 gap-3">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="project-view-start-date">Start date</Label>
          <DateInput
            id="project-view-start-date"
            value={startDate}
            onChange={(e) => onChangeStartDate(e.target.value)}
            data-testid="project-view-start-date"
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="project-view-due-date">Due date</Label>
          <DateInput
            id="project-view-due-date"
            value={dueDate}
            onChange={(e) => onChangeDueDate(e.target.value)}
            data-testid="project-view-due-date"
          />
          {dueDate ? (
            <p
              className="text-xs text-muted-foreground"
              data-testid="project-view-due-date-preview"
            >
              {weekOfMonthLabel(dueDate)}
            </p>
          ) : null}
        </div>
      </div>

      <DateDisplayPicker
        value={dateDisplay}
        onChange={onChangeDateDisplay}
        dueDate={dueDate || null}
        hint={
          isBatch
            ? "how the due date appears on every tab in this batch"
            : "how the due date appears on this tab"
        }
      />
    </div>
  );
}
