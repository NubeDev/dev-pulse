/**
 * Account · Settings — per-user K/V settings page.
 *
 * Backed by `GET/PUT/DELETE /me/settings/*` (dp-rest `settings`
 * module). The server returns the full catalogue on every list
 * call (one row per pinned key, joined with the caller's saved
 * value when present), so this page renders a complete settings
 * form without a parallel client-side catalogue.
 *
 * Secret keys (e.g. `github.pat`) are rendered as
 * `<input type="password">` and never receive a value from the
 * server — only `has_value` reveals "is set". Saving a new value
 * upserts; clearing the field and pressing Save deletes the row.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { IconCheck, IconKey, IconPlugConnected, IconTrash } from "@tabler/icons-react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { api } from "@/api/client";
import type { SettingDto, TestGithubPatResponse } from "@/api/client";

const QUERY_KEY = ["me", "settings"] as const;

export function SettingsPage(): JSX.Element {
  const query = useQuery({
    queryKey: QUERY_KEY,
    queryFn: () => api.listSettings(),
  });

  return (
    <div className="container mx-auto max-w-3xl space-y-6 p-6">
      <header>
        <h1 className="text-2xl font-semibold">Settings</h1>
        <p className="text-sm text-muted-foreground">
          Per-user configuration. Values are scoped to your account
          and never shared with other operators.
        </p>
      </header>

      {query.isLoading ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Spinner /> Loading settings…
        </div>
      ) : query.isError ? (
        <Alert variant="destructive">
          <AlertDescription>
            Failed to load settings: {String(query.error)}
          </AlertDescription>
        </Alert>
      ) : (
        <div className="space-y-4">
          {(query.data ?? []).map((s) => (
            <SettingCard key={s.key} setting={s} />
          ))}
        </div>
      )}
    </div>
  );
}

function SettingCard({ setting }: { setting: SettingDto }): JSX.Element {
  const queryClient = useQueryClient();
  // Local draft. For secret keys the server never echoes the
  // value back, so the draft starts empty and the placeholder
  // tells the user a value is already set.
  const [draft, setDraft] = useState<string>(
    setting.is_secret ? "" : setting.value ?? "",
  );
  const [savedNote, setSavedNote] = useState<string | null>(null);

  const putMutation = useMutation({
    mutationFn: (value: string) => api.putSetting(setting.key, value),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: QUERY_KEY });
      setSavedNote("Saved.");
      if (setting.is_secret) setDraft("");
      window.setTimeout(() => setSavedNote(null), 3000);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: () => api.deleteSetting(setting.key),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: QUERY_KEY });
      setDraft("");
      setSavedNote("Removed.");
      window.setTimeout(() => setSavedNote(null), 3000);
    },
  });

  const busy = putMutation.isPending || deleteMutation.isPending;
  const lastError = putMutation.error ?? deleteMutation.error;

  // Connectivity probe for `github.pat`. Both success and failure
  // outcomes come back as HTTP 200 with a discriminated payload
  // (see `TestGithubPatResponseSchema`), so we keep the *result*
  // separately from the mutation `error` (which is reserved for
  // transport failures).
  const [probeResult, setProbeResult] = useState<TestGithubPatResponse | null>(
    null,
  );
  const testMutation = useMutation({
    mutationFn: () => api.testGithubPat(),
    onSuccess: (res) => setProbeResult(res),
    onError: () => setProbeResult(null),
  });

  function handleSave() {
    putMutation.mutate(draft);
  }

  function handleClear() {
    if (setting.has_value) {
      deleteMutation.mutate();
    } else {
      setDraft("");
    }
  }

  return (
    <Card data-testid={`setting-card-${setting.key}`}>
      <CardHeader>
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            {setting.is_secret ? <IconKey size={16} /> : null}
            <CardTitle className="text-base">{setting.label}</CardTitle>
          </div>
          {setting.has_value ? (
            <Badge variant="secondary" className="gap-1">
              <IconCheck size={12} /> Set
            </Badge>
          ) : (
            <Badge variant="outline">Not set</Badge>
          )}
        </div>
        <CardDescription>{setting.help}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="space-y-1.5">
          <Label htmlFor={`setting-${setting.key}`} className="text-xs">
            {setting.key}
          </Label>
          <Input
            id={`setting-${setting.key}`}
            type={setting.is_secret ? "password" : "text"}
            value={draft}
            placeholder={
              setting.is_secret && setting.has_value
                ? "•••••••• (set — type a new value to replace)"
                : ""
            }
            onChange={(e) => setDraft(e.target.value)}
            disabled={busy}
            autoComplete="off"
            spellCheck={false}
          />
        </div>

        {lastError ? (
          <Alert variant="destructive">
            <AlertDescription>{String(lastError)}</AlertDescription>
          </Alert>
        ) : null}

        {setting.key === "github.pat" && setting.has_value ? (
          <GithubPatProbe
            result={probeResult}
            running={testMutation.isPending}
            transportError={testMutation.error}
            onRun={() => testMutation.mutate()}
          />
        ) : null}

        <div className="flex items-center justify-between">
          <p className="text-xs text-muted-foreground">
            {savedNote ??
              (setting.updated_at
                ? `Last updated ${new Date(setting.updated_at).toLocaleString("en-AU")}`
                : "Never set.")}
          </p>
          <div className="flex gap-2">
            {setting.has_value ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleClear}
                disabled={busy}
              >
                <IconTrash size={14} /> Remove
              </Button>
            ) : null}
            <Button
              type="button"
              size="sm"
              onClick={handleSave}
              disabled={busy || draft.length === 0}
            >
              Save
            </Button>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

// ---------------------------------------------------------------------------
// `github.pat` connectivity probe
// ---------------------------------------------------------------------------
// Calls `POST /me/settings/github.pat/test` and renders the
// discriminated result inline under the card. Kept as a small
// dedicated component so the success/failure styling lives near
// the call site without bloating `SettingCard`.

function GithubPatProbe({
  result,
  running,
  transportError,
  onRun,
}: {
  result: TestGithubPatResponse | null;
  running: boolean;
  transportError: unknown;
  onRun: () => void;
}): JSX.Element {
  return (
    <div className="space-y-2 rounded-md border bg-muted/30 p-3">
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs text-muted-foreground">
          Verify the stored token by calling{" "}
          <code className="font-mono">GET /user</code> on api.github.com.
          The token itself is never returned to your browser.
        </p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={onRun}
          disabled={running}
          data-testid="setting-github-pat-test"
        >
          {running ? <Spinner /> : <IconPlugConnected size={14} />}
          Test connection
        </Button>
      </div>

      {transportError ? (
        <Alert variant="destructive">
          <AlertDescription>
            Probe request failed: {String(transportError)}
          </AlertDescription>
        </Alert>
      ) : null}

      {result?.ok === "true" ? (
        <Alert>
          <AlertDescription>
            <strong>OK</strong> — authenticated as{" "}
            <code className="font-mono">{result.login}</code>
            {result.name ? ` (${result.name})` : ""}
            {result.account_type ? ` · ${result.account_type}` : ""}.
          </AlertDescription>
        </Alert>
      ) : null}

      {result?.ok === "false" ? (
        <Alert variant="destructive">
          <AlertDescription>
            <strong>{result.code}</strong> — {result.message}
          </AlertDescription>
        </Alert>
      ) : null}
    </div>
  );
}
