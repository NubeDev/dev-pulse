/**
 * Issues page — SCOPE-PROJECTS §8.
 *
 * Two surfaces share this file:
 *
 * 1. **Create issue** form — `POST /issues`. No `expected_version`
 *    (there is no row to CAS yet). Gated behind the §8.4
 *    `WritesGate` for the selected org.
 *
 * 2. **Edit issue** form — `PATCH /issues/{id}` (and `POST
 *    /issues/{id}/comments`). The CAS-on-version path: the form
 *    loads the issue, captures `version` as `expected_version`, and
 *    on submit either succeeds or surfaces the §8.3 stale-version
 *    reload UX:
 *
 *      "This issue changed since you opened the form. Reload to see
 *      the new state, then re-apply your edit."
 *
 *    The reload re-runs the GET, drops the cached form state, and
 *    re-prompts. The §8.3 contract guarantees the server hands us
 *    `current_version` on the 409 so the reload is single-shot —
 *    we don't need a second GET to learn the new version, just the
 *    new field values.
 *
 * Issue-management UI is intentionally minimal in this stage —
 * everything that matters for "the §8 write path lands in the
 * frontend" is wired here, but per-org repo pickers and a real
 * issue listing view are deferred to a follow-up.
 */

import { useMemo, useState } from "react";
import { IconAlertTriangle, IconRefresh } from "@tabler/icons-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";

import type { IssueDto } from "../api/client.js";
import { PageHeading } from "../components/page-heading.jsx";

import { mockAppInstallBanner, mockIssue, USE_MOCK } from "./mocks.js";
import {
  staleVersionFromError,
  useCommentOnIssue,
  useIssue,
  useUpdateIssue,
  writesUnavailableOrg,
} from "./use-workflow-data.js";
import { WritesGate } from "./writes-banner.js";

export function IssuesPage(): JSX.Element {
  // Stage 11 ships one demo issue end-to-end so the §8.3 UX is
  // wireable from the smoke harness. A full repo+issue picker is
  // deferred to a follow-up stage.
  const issueId = USE_MOCK ? mockIssue.id : null;
  return (
    <div className="flex flex-col gap-6 px-4 lg:px-6" data-testid="issues-page">
      <PageHeading
        title="Issues"
        description="User-initiated GitHub Issues CRUD. Writes go through the §8.2 optimistic-CAS path; the form reloads automatically if the local row drifted (§8.3)."
      />
      {issueId ? <IssueEditCard issueId={issueId} /> : (
        <Alert>
          <AlertTitle>No issue selected</AlertTitle>
          <AlertDescription>
            Pick an issue from a report row to edit, comment on, or close it.
          </AlertDescription>
        </Alert>
      )}
    </div>
  );
}

function IssueEditCard({ issueId }: { issueId: string }): JSX.Element {
  const issue = useIssue(issueId);
  if (issue.isLoading) {
    return <Card><CardContent>Loading issue…</CardContent></Card>;
  }
  if (issue.isError || !issue.data) {
    return (
      <Alert variant="destructive">
        <AlertTitle>Could not load issue</AlertTitle>
        <AlertDescription>
          {issue.error instanceof Error ? issue.error.message : "Unknown"}
        </AlertDescription>
      </Alert>
    );
  }
  return <IssueEditForm issue={issue.data} onReload={() => issue.refetch()} />;
}

/**
 * The actual form. Holds a *form-local* copy of the editable fields
 * keyed by `formKey` — bumping `formKey` on a §8.3 reload drops the
 * controlled state and re-seeds from the latest GET, which is what
 * §8.3 wants ("ask the UI to reload and re-prompt the user").
 */
function IssueEditForm({
  issue,
  onReload,
}: {
  issue: IssueDto;
  onReload: () => void;
}): JSX.Element {
  const orgLogin = useOrgLogin(issue.org_id);
  const [formKey, setFormKey] = useState(0);
  return (
    <Card data-testid="issue-edit-card">
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>
          Issue #{issue.number} · v{issue.version}
        </CardTitle>
        <Badge variant={issue.state === "open" ? "default" : "secondary"}>
          {issue.state}
        </Badge>
      </CardHeader>
      <CardContent>
        <WritesGate orgLogin={orgLogin}>
          <IssueFormBody
            key={`${issue.id}:${issue.version}:${formKey}`}
            issue={issue}
            onStale={() => {
              setFormKey((k) => k + 1);
              onReload();
            }}
          />
        </WritesGate>
      </CardContent>
    </Card>
  );
}

function IssueFormBody({
  issue,
  onStale,
}: {
  issue: IssueDto;
  onStale: () => void;
}): JSX.Element {
  const [title, setTitle] = useState(issue.title);
  const [body, setBody] = useState(issue.body ?? "");
  const [state, setState] = useState(issue.state);
  const [comment, setComment] = useState("");
  const [staleNotice, setStaleNotice] = useState<{ currentVersion: number } | null>(null);
  const [writesNotice, setWritesNotice] = useState<string | null>(null);

  const update = useUpdateIssue(issue.id);
  const addComment = useCommentOnIssue(issue.id);

  const handleStaleVersion = (e: unknown): boolean => {
    const v = staleVersionFromError(e);
    if (v !== undefined) {
      setStaleNotice({ currentVersion: v });
      return true;
    }
    const org = writesUnavailableOrg(e);
    if (org) {
      setWritesNotice(org);
      return true;
    }
    return false;
  };

  const onSubmit = (ev: React.FormEvent): void => {
    ev.preventDefault();
    update.mutate(
      {
        expected_version: issue.version,
        title: title !== issue.title ? title : undefined,
        body: body !== (issue.body ?? "") ? body : undefined,
        state: state !== issue.state ? state : undefined,
      },
      {
        onError: handleStaleVersion,
      },
    );
  };

  const onComment = (ev: React.FormEvent): void => {
    ev.preventDefault();
    if (!comment.trim()) return;
    addComment.mutate(
      { expected_version: issue.version, body: comment },
      {
        onError: handleStaleVersion,
        onSuccess: () => setComment(""),
      },
    );
  };

  const onClose = (): void => {
    update.mutate(
      { expected_version: issue.version, state: "closed" },
      { onError: handleStaleVersion },
    );
  };
  const onReopen = (): void => {
    update.mutate(
      { expected_version: issue.version, state: "open" },
      { onError: handleStaleVersion },
    );
  };

  return (
    <div className="flex flex-col gap-4">
      {staleNotice && (
        <Alert data-testid="stale-version-notice">
          <IconAlertTriangle className="size-4" />
          <AlertTitle>This issue changed since you opened the form</AlertTitle>
          <AlertDescription className="flex flex-col gap-2">
            <span>
              The local row moved from v{issue.version} to v
              {staleNotice.currentVersion} while you were editing. Your draft
              was not applied. Reload to see the latest state, then re-apply.
            </span>
            <div>
              <Button
                size="sm"
                onClick={() => {
                  setStaleNotice(null);
                  onStale();
                }}
                data-testid="stale-version-reload"
              >
                <IconRefresh className="mr-1 size-4" />
                Reload
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      )}
      {writesNotice && (
        <Alert variant="destructive" data-testid="writes-unavailable-error">
          <AlertTitle>Writes not available for {writesNotice}</AlertTitle>
          <AlertDescription>
            The GitHub App install for this org does not have{" "}
            <code>issues: write</code>. Ask an admin to re-consent.
          </AlertDescription>
        </Alert>
      )}
      <form className="flex flex-col gap-3" onSubmit={onSubmit}>
        <input type="hidden" name="expected_version" value={issue.version} />
        <div className="flex flex-col gap-1">
          <Label>Title</Label>
          <Input value={title} onChange={(e) => setTitle(e.target.value)} />
        </div>
        <div className="flex flex-col gap-1">
          <Label>Body</Label>
          <Textarea
            value={body}
            onChange={(e) => setBody(e.target.value)}
            rows={6}
          />
        </div>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? "Saving…" : "Save changes"}
          </Button>
          {state === "open" ? (
            <Button
              type="button"
              variant="outline"
              onClick={onClose}
              disabled={update.isPending}
            >
              Close issue
            </Button>
          ) : (
            <Button
              type="button"
              variant="outline"
              onClick={onReopen}
              disabled={update.isPending}
            >
              Reopen issue
            </Button>
          )}
          <span className="ml-auto text-xs text-muted-foreground">
            expected_version = <code>{issue.version}</code>
          </span>
        </div>
      </form>
      <form className="flex flex-col gap-3 border-t border-border pt-4" onSubmit={onComment}>
        <Label>Add comment</Label>
        <Textarea
          value={comment}
          onChange={(e) => setComment(e.target.value)}
          rows={3}
          placeholder="Write a comment…"
        />
        <div>
          <Button
            type="submit"
            disabled={addComment.isPending || !comment.trim()}
          >
            {addComment.isPending ? "Posting…" : "Comment"}
          </Button>
        </div>
      </form>
    </div>
  );
}

/** Resolve `org_id` → `login` from the banner data so `WritesGate`
 *  can look up the right row. Mock-aware. */
function useOrgLogin(orgId: string): string | undefined {
  return useMemo(() => {
    if (USE_MOCK) {
      return mockAppInstallBanner.orgs.find((o) => o.org_id === orgId)?.login;
    }
    return undefined;
  }, [orgId]);
}

