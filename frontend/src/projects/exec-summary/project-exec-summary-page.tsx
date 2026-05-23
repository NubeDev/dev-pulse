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

import { useMemo, useState } from "react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Spinner } from "@/components/ui/spinner";

import type {
  ExecSummarySectionId,
  ProjectDto,
} from "../../api/client.js";

import { ExecSummaryHeader } from "./exec-summary-header.js";
import { ExecSummaryNav } from "./exec-summary-nav.js";
import {
  useExecSummary,
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
import { SECTIONS, type ExecSummaryPermissions } from "./shared.js";

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
  const [active, setActive] = useState<ExecSummarySectionId>("summary");

  const permissions = useMemo<ExecSummaryPermissions>(() => {
    const isLead =
      viewerUserId !== null &&
      project.lead_user_id !== null &&
      project.lead_user_id !== undefined &&
      project.lead_user_id === viewerUserId;
    return {
      canEdit: true,
      canSubmit: true,
      canApprove: isLead,
      canRevert: isLead,
    };
  }, [project.lead_user_id, viewerUserId]);

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

  if (query.isError) {
    return (
      <Alert variant="destructive" data-testid="exec-summary-error">
        <AlertTitle>Couldn't load exec summary</AlertTitle>
        <AlertDescription>{query.error.message}</AlertDescription>
      </Alert>
    );
  }

  const data = query.data;
  const activeMeta = SECTIONS.find((s) => s.id === active) ?? SECTIONS[0]!;

  return (
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
          />
        </aside>

        <main className="min-w-0 flex-1">
          <Card className="rounded-2xl border-slate-200 shadow-sm">
            <CardHeader>
              <CardTitle className="text-base">{activeMeta.label}</CardTitle>
              <p className="text-xs text-muted-foreground">
                {activeMeta.description}
              </p>
            </CardHeader>
            <CardContent>
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
  );
}
