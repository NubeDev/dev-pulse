/**
 * Project Executive Summary page (SCOPE-PROJECT-EXECUTIVE-SUMMARY.md §4).
 *
 * Mounted from `project-detail-page.tsx` as the "Exec Summary" tab.
 * Owns:
 *  - the dark header (status, completion %, Submit / Approve / Revert)
 *  - the sticky left nav (8 sections, step number or tick)
 *  - the active section's content card
 *
 * Section components are dumb shells that take the loaded DTO and
 * call `useExecSummaryAutosave` directly — keeps this file from
 * ballooning into an 800-line god component, and means a new
 * section can be added without touching the page wiring.
 */

import { useEffect, useMemo, useRef, useState } from "react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/utils";

import type { ProjectDto } from "../../api/client.js";

import { ExecSummaryHeader } from "./exec-summary-header.js";
import { ExecSummaryNav, type ExecSummaryNavId } from "./exec-summary-nav.js";
import {
  useExecSummary,
  useExecSummaryInlineImageUploader,
  usePatchExecSummary,
} from "./hooks/use-exec-summary.js";
import { ApprovalSection } from "./sections/approval-section.js";
import { ChangelogSection } from "./sections/changelog-section.js";
import { CommercialSection } from "./sections/commercial-section.js";
import { DocumentsSection } from "./sections/documents-section.js";
import { HardwareSection } from "./sections/hardware-section.js";
import { RequirementsSection } from "./sections/requirements-section.js";
import { ScopeSection } from "./sections/scope-section.js";
import { SummarySection } from "./sections/summary-section.js";
import { ValidateSection } from "./sections/validate-section.js";
import { SectionNaToggle } from "./section-na-toggle.js";
import {
  ExecSummaryImageUploaderContext,
  SECTIONS,
  type ExecSummaryPermissions,
} from "./shared.js";
import { computeMissingFields } from "./validation.js";

export interface ProjectExecSummaryPageProps {
  project: ProjectDto;
  /** Current viewer's user id — drives the lead-only Approve/Revert gate. */
  viewerUserId: string | null;
}

export function ProjectExecSummaryPage({
  project,
  viewerUserId,
}: ProjectExecSummaryPageProps): JSX.Element {
  const query = useExecSummary(project.id);
  const patchMutation = usePatchExecSummary(project.id);
  const inlineImageUpload = useExecSummaryInlineImageUploader(project.id);
  const [active, setActive] = useState<ExecSummaryNavId>("summary");
  /** When the user clicks a "Fix" / "Open section" button on the
   *  Validate tab we stash the target field key here, then a layout
   *  effect post-render scrolls the matching input into view and
   *  focuses it. Cleared on apply so a later tab switch doesn't
   *  re-scroll. */
  const pendingFocus = useRef<string | null>(null);

  const permissions = useMemo<ExecSummaryPermissions>(() => {
    // Approve/Revert were lead-only, but if no lead is set the
    // summary becomes un-approvable forever. Open it up to anyone
    // who can edit so the workflow isn't blocked by missing
    // lead assignment — server still records the actor in audit.
    return {
      canEdit: true,
      canSubmit: true,
      canApprove: true,
      canRevert: true,
    };
  }, []);

  // Hooks below must stay above any early-return so the call order
  // is stable between renders (React's Rules of Hooks). They guard
  // on `query.data` being undefined while still loading.
  const data = query.data;
  const missingCount = useMemo(
    () => (data ? computeMissingFields(data).length : 0),
    [data],
  );
  // If everything is now complete, snap off the (hidden) Validate
  // tab so we don't render an empty active page.
  useEffect(() => {
    if (active === "validate" && missingCount === 0) {
      setActive("summary");
    }
  }, [active, missingCount]);

  // Post-render: scroll-and-focus a deferred jump target.
  useEffect(() => {
    const key = pendingFocus.current;
    if (key === null || active === "validate") return;
    pendingFocus.current = null;
    // Defer one frame so the freshly-rendered section's inputs are
    // in the DOM before we look them up.
    const raf = requestAnimationFrame(() => {
      const el = document.querySelector<HTMLElement>(
        `[data-validation-key="${CSS.escape(key)}"]`,
      );
      if (!el) return;
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      const focusable = el.querySelector<HTMLElement>(
        "input, textarea, select, button",
      );
      (focusable ?? el).focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [active]);

  if (query.isPending) {
    return (
      <div
        className="flex items-center gap-2 py-6 text-sm text-muted-foreground"
        data-testid="exec-summary-loading"
      >
        <Spinner /> Loading executive summary…
      </div>
    );
  }

  if (query.isError || !data) {
    return (
      <Alert variant="destructive" data-testid="exec-summary-error">
        <AlertTitle>Couldn't load exec summary</AlertTitle>
        <AlertDescription>
          {query.error?.message ?? "Unknown error"}
        </AlertDescription>
      </Alert>
    );
  }

  const handleJumpTo = (
    sectionId: import("../../api/client.js").ExecSummarySectionId,
    fieldKey: string,
  ): void => {
    pendingFocus.current = fieldKey;
    setActive(sectionId);
  };

  const isValidate = active === "validate";
  const activeMeta = isValidate
    ? {
        id: "validate" as const,
        label: "Validate",
        description:
          "Every required field that's still missing, across all sections.",
        step: 0,
      }
    : SECTIONS.find((s) => s.id === active) ?? SECTIONS[0]!;

  return (
    <ExecSummaryImageUploaderContext.Provider value={inlineImageUpload}>
    <div className="flex flex-col gap-4" data-testid="exec-summary-page">
      <ExecSummaryHeader
        projectId={project.id}
        data={data}
        permissions={permissions}
        saving={patchMutation.isPending}
      />

      <div className="flex flex-col gap-4 lg:flex-row">
        <aside className="lg:w-64 lg:shrink-0">
          <ExecSummaryNav
            active={active}
            onSelect={setActive}
            completion={data.completion.sections}
            skipped={data.skipped_sections}
            missingCount={missingCount}
          />
        </aside>

        <main className="min-w-0 flex-1">
          <Card>
            <CardHeader>
              <div className="flex items-start justify-between gap-3">
                <div className="flex flex-col gap-1">
                  <CardTitle className="text-base">{activeMeta.label}</CardTitle>
                  <p className="text-xs text-muted-foreground">
                    {activeMeta.description}
                  </p>
                </div>
                {active !== "validate" && (
                  <SectionNaToggle
                    projectId={project.id}
                    data={data}
                    sectionId={active}
                  />
                )}
              </div>
            </CardHeader>
            <CardContent
              className={cn(
                !isValidate &&
                  data.skipped_sections.includes(active) &&
                  "opacity-60",
              )}
            >
              {active === "validate" && (
                <ValidateSection
                  projectId={project.id}
                  data={data}
                  onJumpTo={handleJumpTo}
                />
              )}
              {active === "summary" && (
                <SummarySection projectId={project.id} data={data} />
              )}
              {active === "scope" && (
                <ScopeSection projectId={project.id} data={data} />
              )}
              {active === "requirements" && (
                <RequirementsSection projectId={project.id} data={data} />
              )}
              {active === "hardware" && (
                <HardwareSection projectId={project.id} data={data} />
              )}
              {active === "commercial" && (
                <CommercialSection projectId={project.id} data={data} />
              )}
              {active === "documents" && (
                <DocumentsSection projectId={project.id} data={data} />
              )}
              {active === "approval" && (
                <ApprovalSection
                  projectId={project.id}
                  data={data}
                  permissions={permissions}
                />
              )}
              {active === "changelog" && (
                <ChangelogSection projectId={project.id} data={data} />
              )}
            </CardContent>
          </Card>
        </main>
      </div>
    </div>
    </ExecSummaryImageUploaderContext.Provider>
  );
}
