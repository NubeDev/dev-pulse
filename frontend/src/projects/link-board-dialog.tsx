/**
 * §6.4 Link-a-board dialog — the primary (and now only) surface
 * for wiring a dev-pulse Project to a GitHub Projects v2 board.
 * Replaces the retired per-repo admin linker.
 *
 * Three controls only:
 *   - Board dropdown   ← `GET /orgs/{org_id}/projects-v2`
 *                       (org-scoped; the project carries its
 *                       `org_id` so there is no second dropdown).
 *   - Start field      ← the selected board's `date_fields`.
 *   - Due field        ← same.
 *
 * **No node-id paste field on this dialog.** Per §3.1 / §6.4 of
 * `linear-projects-v2.md`, the primary path never asks the user
 * to copy `PVT_…` strings out of GitHub. When the picker call
 * returns `null` (transport unconfigured, GraphQL 5xx, token has
 * no `project` scope) the dialog shows an explainer + an
 * `[Open GitHub project settings]` deep link and disables the
 * `[Link board]` button. No paste box surfaces anywhere in the
 * dev-pulse UI — the §9.4 admin escape hatch was retired in
 * stage 11.
 *
 * On submit: POST `/projects/{id}/board-links` with the picker-
 * resolved title / url so the link row renders the right name
 * immediately, without waiting for the nightly metadata refresh.
 */

import { useEffect, useState } from "react";

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
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";

import {
  isDpRestError,
  type BoardPickerDto,
} from "../api/client.js";
import {
  useCreateBoardLink,
  useCreateOrgProjectV2DateField,
  useOrgBoardPicker,
} from "./use-projects-data.js";

const NO_FIELD = "__none__";

export interface LinkBoardDialogProps {
  /** Whether the dialog is open. Controlled — the host page owns
   *  the trigger state so the row that fired it can re-focus on
   *  close. */
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The project being linked. `org_id` scopes the picker. */
  projectId: string;
  projectOrgId: string;
}

export function LinkBoardDialog({
  open,
  onOpenChange,
  projectId,
  projectOrgId,
}: LinkBoardDialogProps): JSX.Element {
  // Only kick off the picker call when the dialog is open — the
  // GraphQL fan-out is non-trivial and we shouldn't fire it on
  // every render of the detail page.
  const picker = useOrgBoardPicker(projectOrgId, open);
  const create = useCreateBoardLink(projectId);
  const createDateField = useCreateOrgProjectV2DateField(projectOrgId);

  const [boardNodeId, setBoardNodeId] = useState<string>("");
  const [startFieldId, setStartFieldId] = useState<string>("");
  const [dueFieldId, setDueFieldId] = useState<string>("");

  // Reset form state when the dialog re-opens so a previous-link
  // attempt's selection doesn't leak into a new one.
  useEffect(() => {
    if (open) {
      setBoardNodeId("");
      setStartFieldId("");
      setDueFieldId("");
      create.reset();
    }
    // We intentionally exclude `create` from the deps — including
    // it would loop on `reset()`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const boards: BoardPickerDto[] = picker.data?.boards ?? [];
  const selectedBoard = boards.find((b) => b.node_id === boardNodeId);
  const dateFields = selectedBoard?.date_fields ?? [];

  // True iff we are in the "picker unavailable" branch (per §6.4
  // — `[Open GitHub project settings]` hint, no paste box).
  const pickerUnavailable =
    !picker.isPending && (picker.data === null || picker.isError);

  const onSubmit = (): void => {
    if (!selectedBoard) return;
    create.mutate(
      {
        github_board_node_id: selectedBoard.node_id,
        github_board_title: selectedBoard.title,
        github_board_url: selectedBoard.url ?? undefined,
        start_field_node_id: startFieldId || undefined,
        due_field_node_id: dueFieldId || undefined,
      },
      {
        onSuccess: () => onOpenChange(false),
      },
    );
  };

  // Surface a clean "this board is already linked" error from the
  // §7.3 409 path. Other errors fall through to the generic alert.
  const createErr: { title: string; body: string } | null = (() => {
    if (!create.error) return null;
    if (isDpRestError(create.error) && create.error.code === "board_already_linked") {
      return {
        title: "Board already linked",
        body: "This project already mirrors to that board. Pick a different board or unlink the existing row first.",
      };
    }
    return {
      title: "Link failed",
      body: create.error.message,
    };
  })();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-lg"
        data-testid="link-board-dialog"
      >
        <DialogHeader>
          <DialogTitle>Link a GitHub board</DialogTitle>
          <DialogDescription>
            Mirror this project's issue dates to a GitHub Projects v2
            board. The picker shows every board visible to dev-pulse
            for this org.
          </DialogDescription>
        </DialogHeader>

        {picker.isPending && (
          <div
            className="flex items-center gap-2 py-4 text-sm text-muted-foreground"
            data-testid="link-board-loading"
          >
            <Spinner /> Loading boards…
          </div>
        )}

        {pickerUnavailable && (
          <Alert data-testid="link-board-picker-unavailable">
            <AlertTitle>GitHub picker unavailable</AlertTitle>
            <AlertDescription>
              dev-pulse can't reach GitHub's Projects v2 API right
              now. The token may be missing the <code>project</code>{" "}
              scope, or the GraphQL transport is unconfigured for
              this deployment. Verify the org's GitHub installation,
              then re-open this dialog.
              <div className="mt-2">
                <a
                  href="https://github.com/settings/installations"
                  target="_blank"
                  rel="noreferrer"
                  className="text-sm font-medium text-primary underline"
                  data-testid="link-board-github-settings"
                >
                  Open GitHub project settings →
                </a>
              </div>
            </AlertDescription>
          </Alert>
        )}

        {!picker.isPending && !pickerUnavailable && boards.length === 0 && (
          <Alert data-testid="link-board-empty">
            <AlertTitle>No boards in this org</AlertTitle>
            <AlertDescription>
              No GitHub Projects v2 boards are visible for this org.
              Create one in GitHub first, then re-open this dialog.
            </AlertDescription>
          </Alert>
        )}

        {!picker.isPending && !pickerUnavailable && boards.length > 0 && (
          <div className="flex flex-col gap-4 py-2">
            <div className="flex flex-col gap-1">
              <Label className="text-sm">Board on GitHub</Label>
              <Select
                value={boardNodeId}
                onValueChange={(v) => {
                  setBoardNodeId(v);
                  // Reset field ids — they belong to the previous
                  // board's schema.
                  setStartFieldId("");
                  setDueFieldId("");
                }}
              >
                <SelectTrigger data-testid="link-board-board-select">
                  <SelectValue placeholder="Select a board" />
                </SelectTrigger>
                <SelectContent>
                  {boards.map((b) => (
                    <SelectItem key={b.node_id} value={b.node_id}>
                      {b.number ? `#${b.number} — ${b.title}` : b.title}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {selectedBoard && (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                <FieldPicker
                  label="dev-pulse Start →"
                  fields={dateFields}
                  value={startFieldId}
                  onChange={setStartFieldId}
                  testId="link-board-start-field"
                />
                <FieldPicker
                  label="dev-pulse Due →"
                  fields={dateFields}
                  value={dueFieldId}
                  onChange={setDueFieldId}
                  testId="link-board-due-field"
                />
              </div>
            )}

            {selectedBoard && (
              <div
                className="flex flex-col gap-2 rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground"
                data-testid="link-board-create-fields"
              >
                <div>
                  Need date fields? dev-pulse can create them on this
                  board so you don't have to leave the app.
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={createDateField.isPending}
                    onClick={async () => {
                      // Create whichever lane is unset; preselect both.
                      const ensureField = async (
                        existingId: string,
                        name: string,
                      ): Promise<string> => {
                        if (existingId) return existingId;
                        const existing = dateFields.find(
                          (f) =>
                            f.name.trim().toLowerCase() ===
                            name.trim().toLowerCase(),
                        );
                        if (existing) return existing.node_id;
                        const r = await createDateField.mutateAsync({
                          project_node_id: selectedBoard.node_id,
                          name,
                        });
                        return r.node_id;
                      };
                      try {
                        const startId = await ensureField(
                          startFieldId,
                          "Start date",
                        );
                        const dueId = await ensureField(
                          dueFieldId,
                          "Due date",
                        );
                        setStartFieldId(startId);
                        setDueFieldId(dueId);
                      } catch {
                        /* surfaced via createDateField.error below */
                      }
                    }}
                    data-testid="link-board-create-fields-button"
                  >
                    {createDateField.isPending
                      ? "Creating…"
                      : "Create ‘Start date’ & ‘Due date’ fields"}
                  </Button>
                  {createDateField.error && (
                    <span
                      className="text-destructive"
                      data-testid="link-board-create-fields-error"
                    >
                      {createDateField.error.message}
                    </span>
                  )}
                </div>
              </div>
            )}
          </div>
        )}

        {createErr && (
          <Alert variant="destructive" data-testid="link-board-error">
            <AlertTitle>{createErr.title}</AlertTitle>
            <AlertDescription>{createErr.body}</AlertDescription>
          </Alert>
        )}

        <DialogFooter>
          <Button
            variant="ghost"
            onClick={() => onOpenChange(false)}
            disabled={create.isPending}
            data-testid="link-board-cancel"
          >
            Cancel
          </Button>
          <Button
            onClick={onSubmit}
            disabled={
              !selectedBoard || create.isPending || pickerUnavailable
            }
            data-testid="link-board-submit"
          >
            {create.isPending ? "Linking…" : "Link board"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Date-field dropdown. Carries a `(none)` row so the mirror can
 *  skip a lane the board doesn't have — e.g. a board with only
 *  a `Target date` and no `Begin date`. */
function FieldPicker({
  label,
  fields,
  value,
  onChange,
  testId,
}: {
  label: string;
  fields: { node_id: string; name: string }[];
  value: string;
  onChange: (v: string) => void;
  testId: string;
}): JSX.Element {
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
            <SelectItem key={f.node_id} value={f.node_id}>
              {f.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
