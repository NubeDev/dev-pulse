/**
 * §6.3 Project ↔ repos card.
 *
 * Renders the soft repo associations for a project. Used by the
 * §6.3 add-issues dialog to narrow the issue picker to repos the
 * operator has explicitly tied to the project. Does NOT gate
 * membership — issues from non-linked repos can still be added
 * directly.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";

import { api, type RepoSummaryDto } from "../api/client.js";

import {
  useAddProjectRepo,
  useProjectRepos,
  useRemoveProjectRepo,
} from "./use-projects-data.js";

export function ProjectReposCard({
  projectId,
  projectOrgId,
}: {
  projectId: string;
  projectOrgId: string;
}): JSX.Element {
  const links = useProjectRepos(projectId);
  const add = useAddProjectRepo(projectId);
  const remove = useRemoveProjectRepo(projectId);

  const [search, setSearch] = useState("");
  const [pickerOpen, setPickerOpen] = useState(false);

  const reposQ = useQuery({
    queryKey: ["repos", "for-project-link", projectOrgId, search.trim()],
    queryFn: () =>
      api.listRepos({
        org_id: projectOrgId,
        q: search.trim() || undefined,
        limit: 50,
      }),
    enabled: pickerOpen,
    staleTime: 30_000,
  });

  const linkedIds = useMemo(
    () => new Set((links.data ?? []).map((r) => r.repo_id)),
    [links.data],
  );

  const candidates: RepoSummaryDto[] = (reposQ.data?.rows ?? []).filter(
    (r) => !linkedIds.has(r.id),
  );

  return (
    <Card data-testid="project-repos-card">
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle className="text-base">Linked repos</CardTitle>
        <Button
          size="sm"
          onClick={() => setPickerOpen((v) => !v)}
          data-testid="project-repos-add-toggle"
        >
          {pickerOpen ? "Done" : "+ Add repo"}
        </Button>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {links.isPending && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner /> Loading linked repos…
          </div>
        )}

        {links.isError && (
          <Alert variant="destructive">
            <AlertTitle>Couldn't load linked repos</AlertTitle>
            <AlertDescription>{links.error.message}</AlertDescription>
          </Alert>
        )}

        {!links.isPending && (links.data ?? []).length === 0 && !pickerOpen && (
          <p className="text-sm text-muted-foreground">
            No repos linked yet. Linking a repo narrows the §6.3 issue picker
            to issues from that repo — issues from other repos can still be
            added directly.
          </p>
        )}

        {(links.data ?? []).length > 0 && (
          <ul className="flex flex-wrap gap-2" data-testid="project-repos-list">
            {(links.data ?? []).map((row) => (
              <li
                key={row.repo_id}
                className="flex items-center gap-2 rounded-md border border-border bg-muted/30 px-2 py-1 text-xs"
                data-testid="project-repos-row"
              >
                <span className="font-mono">{row.repo_name}</span>
                <button
                  type="button"
                  className="text-muted-foreground hover:text-destructive"
                  disabled={remove.isPending}
                  onClick={() => remove.mutate(row.repo_id)}
                  data-testid="project-repos-remove"
                  aria-label={`Unlink ${row.repo_name}`}
                >
                  ✕
                </button>
              </li>
            ))}
          </ul>
        )}

        {pickerOpen && (
          <div className="flex flex-col gap-2 rounded-md border border-dashed border-border p-3">
            <Input
              placeholder="Search repos by name…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              data-testid="project-repos-search"
            />
            {reposQ.isPending && (
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <Spinner /> Loading repos…
              </div>
            )}
            {reposQ.isError && (
              <Alert variant="destructive">
                <AlertTitle>Couldn't load repos</AlertTitle>
                <AlertDescription>{reposQ.error.message}</AlertDescription>
              </Alert>
            )}
            {!reposQ.isPending && !reposQ.isError && candidates.length === 0 && (
              <p className="text-xs text-muted-foreground">
                No matching unlinked repos.
              </p>
            )}
            <ul className="max-h-48 overflow-y-auto">
              {candidates.map((r) => (
                <li
                  key={r.id}
                  className="flex items-center justify-between border-b border-border/40 py-1 text-sm last:border-b-0"
                >
                  <span className="font-mono text-xs">{r.slug}</span>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={add.isPending}
                    onClick={() => add.mutate(r.id)}
                    data-testid="project-repos-add-row"
                  >
                    Link
                  </Button>
                </li>
              ))}
            </ul>
            {add.error && (
              <Alert variant="destructive">
                <AlertTitle>Couldn't link repo</AlertTitle>
                <AlertDescription>{add.error.message}</AlertDescription>
              </Alert>
            )}
            {remove.error && (
              <Alert variant="destructive">
                <AlertTitle>Couldn't unlink repo</AlertTitle>
                <AlertDescription>{remove.error.message}</AlertDescription>
              </Alert>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
