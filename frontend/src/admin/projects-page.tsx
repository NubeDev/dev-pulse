/**
 * Admin · Projects page — SCOPE-PROJECTS §3.10 repo → GitHub
 * Projects v2 board linker.
 *
 * The §3.10 best-effort mirror needs three node ids per linked
 * repo: the board (`project_node_id`) plus optional Start / Due
 * `dateField` ids. We surface them via the picker (`GET
 * /repos/{id}/projects`) when the deployment has a GraphQL
 * transport wired, and fall back to raw text inputs otherwise so
 * the operator can paste ids from `gh project list --format=json`.
 *
 * Layout: PageHeading + a two-column form Card. Left column picks a
 * repo (autocomplete-flavoured `<Select>`). Right column shows the
 * current link row + the board / Start / Due selectors. Save is
 * `PUT`, Unlink is `DELETE`, both invalidate the link query.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

import { api } from "../api/client.js";
import type { RepoProjectLinkDto } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";

/** Decoded `projectsV2` envelope. Loose — `getRepoProjects` returns
 *  `unknown` so the picker never panics if GitHub adds a new field;
 *  we walk it defensively here. */
interface ProjectV2Node {
  id: string;
  title: string;
  number: number;
  fields: { id: string; name: string; dataType?: string }[];
}

function decodeProjects(raw: unknown): ProjectV2Node[] {
  if (raw == null || typeof raw !== "object") return [];
  // The server returns the raw GraphQL `projectsV2` envelope; it is
  // typically shaped as `{ repository: { projectsV2: { nodes: [...] }}}`
  // but we walk a couple of common shapes to be tolerant.
  const obj = raw as Record<string, unknown>;
  const root =
    (obj.repository as { projectsV2?: { nodes?: unknown[] } } | undefined)
      ?.projectsV2?.nodes ??
    (obj.projectsV2 as { nodes?: unknown[] } | undefined)?.nodes ??
    (obj.nodes as unknown[] | undefined) ??
    [];
  if (!Array.isArray(root)) return [];
  const out: ProjectV2Node[] = [];
  for (const n of root) {
    if (!n || typeof n !== "object") continue;
    const node = n as Record<string, unknown>;
    const id = typeof node.id === "string" ? node.id : "";
    const title = typeof node.title === "string" ? node.title : "(untitled)";
    const number = typeof node.number === "number" ? node.number : 0;
    if (!id) continue;
    const fieldsRoot =
      (node.fields as { nodes?: unknown[] } | undefined)?.nodes ??
      (node.fields as unknown[] | undefined) ??
      [];
    const fields: ProjectV2Node["fields"] = [];
    if (Array.isArray(fieldsRoot)) {
      for (const f of fieldsRoot) {
        if (!f || typeof f !== "object") continue;
        const fo = f as Record<string, unknown>;
        const fid = typeof fo.id === "string" ? fo.id : "";
        const fname = typeof fo.name === "string" ? fo.name : "";
        const dataType =
          typeof fo.dataType === "string" ? fo.dataType : undefined;
        if (!fid || !fname) continue;
        fields.push({ id: fid, name: fname, dataType });
      }
    }
    out.push({ id, title, number, fields });
  }
  return out;
}

const NO_FIELD = "__none__";

export function ProjectsPage(): JSX.Element {
  const qc = useQueryClient();
  const [repoId, setRepoId] = useState<string>("");

  const repos = useQuery({
    queryKey: ["admin", "projects", "repos"],
    queryFn: () => api.listRepos({ limit: 200 }),
    staleTime: 60_000,
  });

  const link = useQuery({
    queryKey: ["admin", "projects", "link", repoId],
    queryFn: () => api.getRepoProjectLink(repoId),
    enabled: !!repoId,
  });

  const picker = useQuery({
    queryKey: ["admin", "projects", "picker", repoId],
    queryFn: () => api.getRepoProjects(repoId),
    enabled: !!repoId,
    retry: false,
  });

  const [projectNodeId, setProjectNodeId] = useState<string>("");
  const [startFieldId, setStartFieldId] = useState<string>("");
  const [dueFieldId, setDueFieldId] = useState<string>("");

  // Seed the form from the server row whenever the link query
  // resolves for a new repo.
  const linkRow: RepoProjectLinkDto | null | undefined = link.data;
  const seedFor = `${repoId}:${linkRow?.project_node_id ?? ""}`;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useStateSeed(seedFor, () => {
    setProjectNodeId(linkRow?.project_node_id ?? "");
    setStartFieldId(linkRow?.start_field_node_id ?? "");
    setDueFieldId(linkRow?.due_field_node_id ?? "");
  });

  const pickerProjects = picker.data ? decodeProjects(picker.data) : [];
  const selectedProject = pickerProjects.find((p) => p.id === projectNodeId);
  const dateFields = selectedProject
    ? selectedProject.fields.filter(
        (f) => f.dataType === undefined || f.dataType === "DATE",
      )
    : [];

  const save = useMutation({
    mutationFn: () =>
      api.putRepoProjectLink(repoId, {
        project_node_id: projectNodeId,
        start_field_node_id: startFieldId || null,
        due_field_node_id: dueFieldId || null,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["admin", "projects", "link", repoId] });
    },
  });

  const unlink = useMutation({
    mutationFn: () => api.deleteRepoProjectLink(repoId),
    onSuccess: () => {
      setProjectNodeId("");
      setStartFieldId("");
      setDueFieldId("");
      qc.invalidateQueries({ queryKey: ["admin", "projects", "link", repoId] });
    },
  });

  return (
    <div className="flex flex-col gap-4">
      <PageHeading
        title="Projects v2 link"
        description="Link a repo to a GitHub Projects v2 board so the date editor mirrors Start / Due fields."
      />

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Repo</CardTitle>
        </CardHeader>
        <CardContent>
          <Label className="text-sm">Pick a repo</Label>
          <Select
            value={repoId}
            onValueChange={(v) => setRepoId(v)}
            disabled={repos.isPending}
          >
            <SelectTrigger
              className="w-full md:w-96"
              data-testid="admin-projects-repo-select"
            >
              <SelectValue
                placeholder={repos.isPending ? "Loading…" : "Select a repo"}
              />
            </SelectTrigger>
            <SelectContent>
              {repos.data?.rows.map((r) => (
                <SelectItem key={r.id} value={r.id}>
                  {r.org_login}/{r.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </CardContent>
      </Card>

      {repoId && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Project link</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            {picker.data === null && (
              <Alert>
                <AlertTitle>No picker available</AlertTitle>
                <AlertDescription>
                  This deployment has no GraphQL transport wired. Paste node ids
                  from <code>gh project list --format=json</code> below.
                </AlertDescription>
              </Alert>
            )}

            {pickerProjects.length > 0 ? (
              <div className="flex flex-col gap-1">
                <Label className="text-sm">Board</Label>
                <Select
                  value={projectNodeId}
                  onValueChange={(v) => {
                    setProjectNodeId(v);
                    // Reset field ids when the board changes — the
                    // old field ids belong to the previous board.
                    setStartFieldId("");
                    setDueFieldId("");
                  }}
                >
                  <SelectTrigger
                    className="w-full md:w-96"
                    data-testid="admin-projects-board-select"
                  >
                    <SelectValue placeholder="Select a board" />
                  </SelectTrigger>
                  <SelectContent>
                    {pickerProjects.map((p) => (
                      <SelectItem key={p.id} value={p.id}>
                        #{p.number} — {p.title}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            ) : (
              <div className="flex flex-col gap-1">
                <Label className="text-sm" htmlFor="project-node-id">
                  Project node id
                </Label>
                <Input
                  id="project-node-id"
                  value={projectNodeId}
                  onChange={(e) => setProjectNodeId(e.target.value)}
                  placeholder="PVT_kwDOA…"
                  className="w-full md:w-96 font-mono"
                  data-testid="admin-projects-project-input"
                />
              </div>
            )}

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <FieldPicker
                label="Start field"
                fields={dateFields}
                value={startFieldId}
                onChange={setStartFieldId}
                fallbackPlaceholder="PVTF_…"
                hasPicker={pickerProjects.length > 0}
                testId="admin-projects-start-field"
              />
              <FieldPicker
                label="Due field"
                fields={dateFields}
                value={dueFieldId}
                onChange={setDueFieldId}
                fallbackPlaceholder="PVTF_…"
                hasPicker={pickerProjects.length > 0}
                testId="admin-projects-due-field"
              />
            </div>

            <div className="flex flex-wrap gap-2 pt-2">
              <Button
                onClick={() => save.mutate()}
                disabled={!projectNodeId || save.isPending}
                data-testid="admin-projects-save"
              >
                {save.isPending ? "Saving…" : "Save link"}
              </Button>
              <Button
                variant="ghost"
                onClick={() => unlink.mutate()}
                disabled={!linkRow || unlink.isPending}
                data-testid="admin-projects-unlink"
              >
                {unlink.isPending ? "Unlinking…" : "Unlink"}
              </Button>
            </div>

            {save.error instanceof Error && (
              <Alert variant="destructive">
                <AlertTitle>Save failed</AlertTitle>
                <AlertDescription>{save.error.message}</AlertDescription>
              </Alert>
            )}
            {unlink.error instanceof Error && (
              <Alert variant="destructive">
                <AlertTitle>Unlink failed</AlertTitle>
                <AlertDescription>{unlink.error.message}</AlertDescription>
              </Alert>
            )}
            {linkRow && !save.isPending && (
              <p className="text-xs text-muted-foreground">
                Linked since {new Date(linkRow.created_at).toLocaleString()} ·
                last updated {new Date(linkRow.updated_at).toLocaleString()}.
              </p>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}

/** Either a `<Select>` over the picker's dateFields or a raw text
 *  input — gracefully degrades when the deployment has no GraphQL
 *  transport. `NO_FIELD` is a Radix-friendly sentinel for "clear". */
function FieldPicker({
  label,
  fields,
  value,
  onChange,
  fallbackPlaceholder,
  hasPicker,
  testId,
}: {
  label: string;
  fields: { id: string; name: string }[];
  value: string;
  onChange: (v: string) => void;
  fallbackPlaceholder: string;
  hasPicker: boolean;
  testId: string;
}): JSX.Element {
  if (!hasPicker) {
    return (
      <div className="flex flex-col gap-1">
        <Label className="text-sm">{label}</Label>
        <Input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={fallbackPlaceholder}
          className="font-mono"
          data-testid={testId}
        />
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1">
      <Label className="text-sm">{label}</Label>
      <Select
        value={value || NO_FIELD}
        onValueChange={(v) => onChange(v === NO_FIELD ? "" : v)}
      >
        <SelectTrigger data-testid={testId}>
          <SelectValue placeholder="(none)" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={NO_FIELD}>(none)</SelectItem>
          {fields.map((f) => (
            <SelectItem key={f.id} value={f.id}>
              {f.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

/** Tiny hook that runs `seed` whenever `key` changes — used to
 *  reset the form when the operator picks a different repo. Avoids
 *  the manual `useEffect(..., [key])` ceremony at every callsite. */
function useStateSeed(key: string, seed: () => void): void {
  // eslint-disable-next-line react-hooks/rules-of-hooks
  const [lastKey, setLastKey] = useState<string | null>(null);
  if (lastKey !== key) {
    setLastKey(key);
    seed();
  }
}
