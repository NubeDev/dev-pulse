/**
 * §6.2 `[+ New project]` modal. Three required-ish fields — name,
 * org, and optional description / start / due — mirroring
 * `CreateProjectRequest`. `status` defaults server-side to
 * `active`; the lead is left blank (the §6.3 detail header is the
 * surface for setting / changing the lead since picking a user
 * needs the org-scoped people directory the create modal does not
 * want to drag in).
 *
 * On success the parent invalidates the projects cache and (when
 * `onCreated` is supplied) the host page navigates to the new
 * project's detail route so the user can immediately add issues.
 */

import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { DateInput } from "@/components/ui/date-input";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { Textarea } from "@/components/ui/textarea";

import { api, type OrgDto, type ProjectDto } from "../api/client.js";

import { useCreateProject } from "./use-projects-data.js";

export interface NewProjectDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Pre-select an org id (e.g. the org currently filtered in the
   *  sidebar). When `null`, the first visible org is picked. */
  defaultOrgId?: string | null;
  /** Called after a successful create, with the new project row. */
  onCreated?: (project: ProjectDto) => void;
}

export function NewProjectDialog({
  open,
  onOpenChange,
  defaultOrgId,
  onCreated,
}: NewProjectDialogProps): JSX.Element {
  const orgsQ = useQuery<OrgDto[]>({
    queryKey: ["orgs"],
    queryFn: () => api.listOrgs(),
    enabled: open,
    staleTime: 5 * 60_000,
  });
  const create = useCreateProject();

  const [name, setName] = useState("");
  const [orgId, setOrgId] = useState<string>("");
  const [description, setDescription] = useState("");
  const [startAt, setStartAt] = useState("");
  const [dueAt, setDueAt] = useState("");

  useEffect(() => {
    if (!open) return;
    setName("");
    setDescription("");
    setStartAt("");
    setDueAt("");
    create.reset();
    if (defaultOrgId) {
      setOrgId(defaultOrgId);
    } else {
      setOrgId(orgsQ.data?.[0]?.id ?? "");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, defaultOrgId]);

  // Once orgs land, fall through to the first row if nothing's set.
  useEffect(() => {
    if (open && !orgId && orgsQ.data && orgsQ.data.length > 0) {
      setOrgId(orgsQ.data[0]!.id);
    }
  }, [open, orgId, orgsQ.data]);

  const orgs = orgsQ.data ?? [];
  const canSubmit =
    name.trim().length > 0 && orgId.length > 0 && !create.isPending;

  const toIso = (v: string): string | undefined => {
    if (!v) return undefined;
    // `<input type="date">` produces `YYYY-MM-DD`; expand to UTC
    // midnight so the server's `DateTime<Utc>` parser accepts it.
    return `${v}T00:00:00Z`;
  };

  const onSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!canSubmit) return;
    create.mutate(
      {
        org_id: orgId,
        name: name.trim(),
        description: description.trim() ? description.trim() : null,
        start_at: toIso(startAt) ?? null,
        due_at: toIso(dueAt) ?? null,
      },
      {
        onSuccess: (project) => {
          onCreated?.(project);
          onOpenChange(false);
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-lg"
        data-testid="new-project-dialog"
      >
        <DialogHeader>
          <DialogTitle>New project</DialogTitle>
          <DialogDescription>
            Projects group issues across repos in the same org.
            You can add issues, set a lead and start / due dates,
            and (optionally) mirror dates to a GitHub Projects v2
            board after the project is created.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
          <div className="flex flex-col gap-2">
            <Label htmlFor="new-project-name">Name</Label>
            <Input
              id="new-project-name"
              data-testid="new-project-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Rubix v2 launch"
              maxLength={200}
              autoFocus
              required
            />
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="new-project-org">Org</Label>
            {orgsQ.isPending ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Spinner /> Loading orgs…
              </div>
            ) : (
              <Select value={orgId} onValueChange={setOrgId}>
                <SelectTrigger
                  id="new-project-org"
                  data-testid="new-project-org"
                >
                  <SelectValue placeholder="Select an org" />
                </SelectTrigger>
                <SelectContent>
                  {orgs.map((o) => (
                    <SelectItem key={o.id} value={o.id}>
                      {o.name ? `${o.name} (${o.login})` : o.login}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="new-project-description">Description</Label>
            <Textarea
              id="new-project-description"
              data-testid="new-project-description"
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="What does this project deliver?"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-2">
              <Label htmlFor="new-project-start">Start</Label>
              <DateInput
                id="new-project-start"
                data-testid="new-project-start"
                value={startAt}
                onChange={(e) => setStartAt(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="new-project-due">Due</Label>
              <DateInput
                id="new-project-due"
                data-testid="new-project-due"
                value={dueAt}
                onChange={(e) => setDueAt(e.target.value)}
              />
            </div>
          </div>

          {create.isError && (
            <Alert variant="destructive" data-testid="new-project-error">
              <AlertTitle>Create failed</AlertTitle>
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
            <Button
              type="submit"
              data-testid="new-project-submit"
              disabled={!canSubmit}
            >
              {create.isPending ? "Creating…" : "Create project"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
