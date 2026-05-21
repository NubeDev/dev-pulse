/**
 * §6.3 Project ↔ repos card.
 *
 * Renders the soft repo associations for a project. Used by the
 * §6.3 add-issues dialog to narrow the issue picker to repos the
 * operator has explicitly tied to the project. Does NOT gate
 * membership — issues from non-linked repos can still be added
 * directly.
 *
 * The card is read-only by default: a small settings (gear) icon
 * in the header opens a Dialog where the operator can search,
 * link, and unlink repos. This mirrors the link-board card pattern
 * but keeps the picker out of the page until requested.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { SettingsIcon, XIcon } from "lucide-react";

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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
}: {
  projectId: string;
}): JSX.Element {
  const links = useProjectRepos(projectId);

  const linkedCount = (links.data ?? []).length;

  return (
    <Card data-testid="project-repos-card">
      <CardHeader className="flex flex-row items-center justify-between">
        <div className="flex items-center gap-2">
          <CardTitle className="text-base">Linked repos</CardTitle>
          {linkedCount > 0 && (
            <span className="text-xs text-muted-foreground">
              ({linkedCount})
            </span>
          )}
        </div>
      </CardHeader>
      <CardContent>
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

        {!links.isPending && linkedCount === 0 && (
          <p className="text-sm text-muted-foreground">
            No repos linked yet. Use{" "}
            <span className="font-medium">Settings → Manage repos…</span>{" "}
            to link repos — linking narrows the §6.3 issue picker to those
            repos (issues from other repos can still be added directly).
          </p>
        )}

        {linkedCount > 0 && (
          <ul
            className="flex flex-wrap gap-2"
            data-testid="project-repos-list"
          >
            {(links.data ?? []).map((row) => (
              <li
                key={row.repo_id}
                className="rounded-md border border-border bg-muted/30 px-2 py-1 font-mono text-xs"
                data-testid="project-repos-row"
                title={`Linked ${new Date(row.added_at).toLocaleString()}`}
              >
                {row.repo_name}
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

export function ManageReposDialog({
  open,
  onOpenChange,
  projectId,
  projectOrgId,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  projectId: string;
  projectOrgId: string;
}): JSX.Element {
  const links = useProjectRepos(open ? projectId : null);
  const add = useAddProjectRepo(projectId);
  const remove = useRemoveProjectRepo(projectId);

  const [search, setSearch] = useState("");

  const reposQ = useQuery({
    queryKey: ["repos", "for-project-link", projectOrgId, search.trim()],
    queryFn: () =>
      api.listRepos({
        org_id: projectOrgId,
        q: search.trim() || undefined,
        limit: 50,
      }),
    enabled: open,
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
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-xl max-h-[85vh] flex flex-col overflow-hidden"
        data-testid="project-repos-dialog"
      >
        <DialogHeader>
          <DialogTitle>Manage linked repos</DialogTitle>
          <DialogDescription>
            Linking a repo scopes the issue picker on this project to that
            repo by default. Cross-org repos are rejected.
          </DialogDescription>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-4">
          <section className="flex flex-col gap-2">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              Currently linked ({(links.data ?? []).length})
            </h3>
            {links.isPending && (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Spinner /> Loading…
              </div>
            )}
            {(links.data ?? []).length === 0 && !links.isPending && (
              <p className="text-sm text-muted-foreground">
                Nothing linked yet.
              </p>
            )}
            {(links.data ?? []).length > 0 && (
              <ul className="flex flex-wrap gap-2">
                {(links.data ?? []).map((row) => (
                  <li
                    key={row.repo_id}
                    className="flex items-center gap-1 rounded-md border border-border bg-muted/30 px-2 py-1 text-xs"
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
                      <XIcon className="h-3 w-3" />
                    </button>
                  </li>
                ))}
              </ul>
            )}
            {remove.error && (
              <Alert variant="destructive">
                <AlertTitle>Couldn't unlink repo</AlertTitle>
                <AlertDescription>{remove.error.message}</AlertDescription>
              </Alert>
            )}
          </section>

          <section className="flex min-h-0 flex-1 flex-col gap-2">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              Add a repo
            </h3>
            <Input
              placeholder="Search repos by name…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              data-testid="project-repos-search"
            />
            <div className="min-h-0 flex-1 overflow-y-auto rounded-md border border-border">
              {reposQ.isPending && (
                <div className="flex items-center gap-2 px-3 py-4 text-sm text-muted-foreground">
                  <Spinner /> Loading repos…
                </div>
              )}
              {reposQ.isError && (
                <Alert variant="destructive" className="m-2">
                  <AlertTitle>Couldn't load repos</AlertTitle>
                  <AlertDescription>{reposQ.error.message}</AlertDescription>
                </Alert>
              )}
              {!reposQ.isPending &&
                !reposQ.isError &&
                candidates.length === 0 && (
                  <p className="px-3 py-6 text-center text-sm text-muted-foreground">
                    No matching unlinked repos.
                  </p>
                )}
              {candidates.map((r) => (
                <div
                  key={r.id}
                  className="flex items-center justify-between border-b border-border/40 px-3 py-2 text-sm last:border-b-0"
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
                </div>
              ))}
            </div>
            {add.error && (
              <Alert variant="destructive">
                <AlertTitle>Couldn't link repo</AlertTitle>
                <AlertDescription>{add.error.message}</AlertDescription>
              </Alert>
            )}
          </section>
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={() => onOpenChange(false)}
          >
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
