/**
 * "Writes not available" banner — SCOPE-PROJECTS §8.4 + §13.6.
 *
 * Two rendering modes flow from the §13.6 banner endpoint:
 *
 * 1. **Deployment hard-disable.** `request_issues_write === false`.
 *    The §8 surface is off deployment-wide. One static banner; no
 *    per-org breakdown.
 * 2. **Per-org migration prompt.** `request_issues_write === true`
 *    but one or more of the viewer's orgs has an install that
 *    pre-dates the §13.6 manifest bump. We render one row per
 *    affected org, each carrying the deep-link to the install's
 *    permissions page and a copy-able admin-text snippet the
 *    viewer can paste into Slack / email — exactly the §13.6
 *    "one-shot prompt" surface.
 *
 * The banner is dismissible per §13.6 ("persistent (dismissible)
 * banner"); dismissal is stored in `localStorage` keyed by the set
 * of read-only org logins so that re-consenting (which removes
 * the org from the list) re-shows the banner the next time a *new*
 * org appears read-only.
 *
 * The same hook (`useAppInstallBanner`) drives the per-issue-form
 * "writes not available for `org-x`" affordance (`WritesGate`
 * below) — there's exactly one place that knows whether a write
 * will succeed.
 */

import { useEffect, useMemo, useState } from "react";
import { IconAlertTriangle, IconCopy, IconExternalLink, IconX } from "@tabler/icons-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";

import type { AppInstallBannerOrgDto } from "../api/client.js";
import { useAppInstallBanner } from "./use-workflow-data.js";

const DISMISS_KEY = "dp:writes-banner:dismissed-set";

/**
 * Top-of-app banner — render in the app shell once, above the
 * page-heading row. Hidden when there's nothing to show or the user
 * has dismissed the banner for the current read-only org set.
 */
export function WritesBanner(): JSX.Element | null {
  const banner = useAppInstallBanner();
  const data = banner.data;
  const readOnlyOrgs = useMemo<AppInstallBannerOrgDto[]>(() => {
    if (!data) return [];
    return data.orgs.filter((o) => !o.writes_available);
  }, [data]);
  const dismissalKey = readOnlyOrgs
    .map((o) => o.login)
    .sort()
    .join("|");
  const [dismissed, setDismissed] = useState<string | null>(null);

  useEffect(() => {
    try {
      setDismissed(localStorage.getItem(DISMISS_KEY));
    } catch {
      setDismissed(null);
    }
  }, []);

  if (!data) return null;
  if (!data.request_issues_write) {
    return (
      <Alert
        data-testid="writes-disabled-banner"
        className="border-amber-300/40 bg-amber-100/30 dark:bg-amber-950/30"
      >
        <IconAlertTriangle className="size-4" />
        <AlertTitle>Writes disabled in this deployment</AlertTitle>
        <AlertDescription>
          The dev-pulse GitHub App is configured without
          <code className="mx-1 rounded bg-muted px-1 py-0.5 text-xs">issues: write</code>
          (<code>github.app.request_issues_write = false</code>). Issue create
          / edit / comment is unavailable; tags and pins still work.
        </AlertDescription>
      </Alert>
    );
  }

  if (readOnlyOrgs.length === 0) return null;
  if (dismissed === dismissalKey) return null;

  const onDismiss = (): void => {
    try {
      localStorage.setItem(DISMISS_KEY, dismissalKey);
    } catch {
      // localStorage may be unavailable (private mode, SSR shim) —
      // dismissal silently reverts to per-session state.
    }
    setDismissed(dismissalKey);
  };

  return (
    <Alert
      data-testid="writes-not-available-banner"
      className="border-amber-300/40 bg-amber-100/30 dark:bg-amber-950/30"
    >
      <IconAlertTriangle className="size-4" />
      <AlertTitle className="flex items-center justify-between gap-2">
        <span>
          Writes not available for {plural(readOnlyOrgs.length, "org", "orgs")}
        </span>
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss"
          className="opacity-60 hover:opacity-100"
        >
          <IconX className="size-4" />
        </button>
      </AlertTitle>
      <AlertDescription>
        <ul className="flex flex-col gap-3 pt-2">
          {readOnlyOrgs.map((org) => (
            <li
              key={org.org_id}
              className="flex flex-col gap-1 rounded-md border border-border/50 bg-background/40 p-3"
              data-testid={`writes-banner-row-${org.login}`}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="font-medium">{org.login}</span>
                {org.manage_url && (
                  <a
                    href={org.manage_url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-1 text-xs text-primary hover:underline"
                  >
                    <IconExternalLink className="size-3" />
                    Open install permissions
                  </a>
                )}
              </div>
              <p className="text-xs text-muted-foreground">
                Issue create / edit / comment is disabled until an admin
                re-consents to the GitHub App. Paste the message below to
                ask:
              </p>
              <CopyableText text={org.admin_copy_text} />
            </li>
          ))}
        </ul>
      </AlertDescription>
    </Alert>
  );
}

function plural(n: number, one: string, many: string): string {
  return n === 1 ? `1 ${one}` : `${n} ${many}`;
}

function CopyableText({ text }: { text: string }): JSX.Element {
  const [copied, setCopied] = useState(false);
  const onCopy = async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // best-effort; nothing to do.
    }
  };
  return (
    <div className="flex items-start gap-2 rounded border border-border/50 bg-muted/50 p-2 text-xs">
      <code className="flex-1 whitespace-pre-wrap break-words">{text}</code>
      <Button
        type="button"
        size="sm"
        variant="outline"
        onClick={onCopy}
        className="shrink-0"
      >
        <IconCopy className="mr-1 size-3" />
        {copied ? "Copied" : "Copy"}
      </Button>
    </div>
  );
}

/**
 * Per-form gate — wraps the children with a §8.4 "writes not
 * available for `<org>`" affordance when the viewer's selected org
 * is read-only. The wrapped children stay rendered (so the form's
 * fields are inspectable) but become non-interactive: every form
 * control sits inside a `fieldset[disabled]`, and the submit button
 * is replaced by a hover-explained disabled chip.
 *
 * Pass `orgLogin` (the targeted write's org login) to scope the
 * gate to a single org; pass `undefined` while the user has not
 * yet picked an org to render a neutral state.
 */
export function WritesGate({
  orgLogin,
  children,
  fallbackTitle = "Writes not available",
}: {
  orgLogin: string | undefined;
  children: React.ReactNode;
  fallbackTitle?: string;
}): JSX.Element {
  const banner = useAppInstallBanner();
  const data = banner.data;

  if (!data || !orgLogin) return <>{children}</>;
  if (!data.request_issues_write) {
    return (
      <DisabledForm>
        <Alert data-testid="writes-gate-deployment-off">
          <IconAlertTriangle className="size-4" />
          <AlertTitle>{fallbackTitle}</AlertTitle>
          <AlertDescription>
            Writes are disabled in this deployment.
          </AlertDescription>
        </Alert>
        {children}
      </DisabledForm>
    );
  }
  const org = data.orgs.find((o) => o.login === orgLogin);
  if (org && !org.writes_available) {
    return (
      <DisabledForm>
        <Alert
          data-testid={`writes-gate-${orgLogin}`}
          className="border-amber-300/40 bg-amber-100/30 dark:bg-amber-950/30"
        >
          <IconAlertTriangle className="size-4" />
          <AlertTitle>Writes not available for {orgLogin}</AlertTitle>
          <AlertDescription>
            This org's GitHub App install was granted read-only.{" "}
            {org.manage_url && (
              <a
                href={org.manage_url}
                target="_blank"
                rel="noopener noreferrer"
                className="text-primary hover:underline"
              >
                Open install permissions
              </a>
            )}
            .
          </AlertDescription>
        </Alert>
        {children}
      </DisabledForm>
    );
  }
  return <>{children}</>;
}

/** Disables every form control inside via a top-level `fieldset`. */
function DisabledForm({ children }: { children: React.ReactNode }): JSX.Element {
  return (
    <fieldset disabled className="flex flex-col gap-3 opacity-90">
      {children}
    </fieldset>
  );
}
