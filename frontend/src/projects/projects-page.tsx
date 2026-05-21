/**
 * `#/projects` — landing for the §6.2 list page.
 *
 * Stage 5 of `linear-projects-v2.md` only ships the sidebar entry +
 * counts; the actual list page (search, status grouping, `[+ New
 * project]` modal) lands in stage 6. Until then we render a small
 * placeholder so the sidebar links land on a real route instead of
 * the 404 page — and so the §6.1 status filter (`?status=…`) round-
 * trips through the URL ahead of the list page picking it up.
 */

import { useRoute } from "../routes.js";
import { projectsStatusOf } from "../routes.js";
import { useProjectCount } from "./use-projects-data.js";

export function ProjectsPage(): JSX.Element {
  const route = useRoute();
  const status = projectsStatusOf(route);
  // Pull the count for the currently-selected status so the page
  // shows _something_ live even before the list lands.
  const probe = useProjectCount(status ?? "active");

  return (
    <div className="px-4 lg:px-6" data-testid="projects-page">
      <div className="rounded-lg border border-dashed border-border bg-muted/30 p-8 text-center">
        <h2 className="text-lg font-semibold">Projects</h2>
        <p className="mt-2 text-sm text-muted-foreground">
          The first-class projects surface lands across the next few
          stages. Slice A wires the sidebar + counts (this stage),
          followed by the list page, the detail page, the workflow
          detail-pane chip, and bulk-add from triage.
        </p>
        <p className="mt-4 text-xs text-muted-foreground">
          Showing status:{" "}
          <code className="rounded bg-background px-1.5 py-0.5">
            {status ?? "(all)"}
          </code>{" "}
          · live count:{" "}
          <span data-testid="projects-page-count">{probe.count}</span>
        </p>
      </div>
    </div>
  );
}
