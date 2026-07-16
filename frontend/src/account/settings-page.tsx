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

// Mirrors `starter_auth_users::signup::validate::DEFAULT_PASSWORD_MIN_LEN`.
const PASSWORD_MIN_LEN = 12;

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

      <PasswordCard />
    </div>
  );
}

/**
 * Change your own password (issue #14). Separate from the K/V
 * settings above: it has its own endpoint (`POST /me/password`), and
 * unlike a setting the value is write-only — there is no `has_value`
 * to read back, because the server stores only an argon2 hash.
 *
 * An account that signs in with GitHub has no local password; the
 * server answers `403 password_not_set` there, which the card
 * surfaces verbatim rather than trying to predict.
 */
function PasswordCard(): JSX.Element {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [done, setDone] = useState(false);

  const mut = useMutation({
    mutationFn: () =>
      api.changeMyPassword({ current_password: current, new_password: next }),
    onSuccess: () => {
      // Never leave the typed secrets sitting in component state.
      setCurrent("");
      setNext("");
      setConfirm("");
      setDone(true);
    },
  });

  const mismatch = confirm.length > 0 && next !== confirm;
  // Mirrors the server minimum; the server also enforces a
  // common-password blocklist and the email-local-part rule, and
  // remains the authority on all three.
  const submittable =
    current.length > 0 && next.length >= PASSWORD_MIN_LEN && next === confirm;

  return (
    <Card data-testid="password-card">
      <CardHeader>
        <div className="flex items-center gap-2">
          <IconKey className="size-4 text-muted-foreground" />
          <CardTitle className="text-base">Password</CardTitle>
        </div>
        <CardDescription>
          Change the password you use to sign in. Your current password is
          required. If you sign in with GitHub, you have no local password —
          ask an operator to set one.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid gap-1.5">
          <Label htmlFor="pw-current">Current password</Label>
          <Input
            id="pw-current"
            data-testid="pw-current"
            type="password"
            autoComplete="current-password"
            value={current}
            onChange={(e) => {
              setCurrent(e.target.value);
              setDone(false);
            }}
          />
        </div>
        <div className="grid gap-1.5">
          <Label htmlFor="pw-new">New password</Label>
          <Input
            id="pw-new"
            data-testid="pw-new"
            type="password"
            autoComplete="new-password"
            value={next}
            onChange={(e) => {
              setNext(e.target.value);
              setDone(false);
            }}
            placeholder={`At least ${PASSWORD_MIN_LEN} characters`}
          />
        </div>
        <div className="grid gap-1.5">
          <Label htmlFor="pw-confirm">Confirm new password</Label>
          <Input
            id="pw-confirm"
            data-testid="pw-confirm"
            type="password"
            autoComplete="new-password"
            value={confirm}
            onChange={(e) => {
              setConfirm(e.target.value);
              setDone(false);
            }}
          />
        </div>

        {mismatch ? (
          <p data-testid="pw-mismatch" className="text-xs text-destructive">
            Passwords do not match.
          </p>
        ) : null}

        {mut.isError ? (
          <Alert variant="destructive" data-testid="pw-error">
            <AlertDescription>
              {mut.error instanceof Error
                ? mut.error.message
                : String(mut.error)}
            </AlertDescription>
          </Alert>
        ) : null}

        {done ? (
          <Alert data-testid="pw-ok">
            <AlertDescription>
              <strong>Password changed.</strong> Your existing sessions stay
              signed in.
            </AlertDescription>
          </Alert>
        ) : null}

        <div className="flex justify-end">
          <Button
            data-testid="pw-save"
            disabled={!submittable || mut.isPending}
            onClick={() => mut.mutate()}
          >
            {mut.isPending ? "Changing…" : "Change password"}
          </Button>
        </div>
      </CardContent>
    </Card>
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
