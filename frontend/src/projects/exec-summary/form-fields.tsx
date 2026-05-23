/**
 * Reusable field controls for the exec-summary form.
 *
 * Each control is a *controlled* component bound to one field of the
 * section payload. The wrapper handles the standard layout (Label +
 * input + optional hint) so the section files stay declarative.
 *
 * `onCommit` is called on blur (or on debounce-flush for the
 * markdown editor) with the new value. Sections feed that into the
 * autosave hook — the input itself doesn't know about react-query.
 */

import { useEffect, useState, type ReactNode } from "react";
import MDEditor from "@uiw/react-md-editor";

import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { DateInput } from "@/components/ui/date-input";
import { Checkbox } from "@/components/ui/checkbox";
import { useTheme } from "@kit/theme";
import { cn } from "@/lib/utils";

interface FieldShellProps {
  id?: string;
  label: string;
  hint?: ReactNode;
  className?: string;
  children: ReactNode;
}

export function FieldShell({
  id,
  label,
  hint,
  className,
  children,
}: FieldShellProps): JSX.Element {
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <Label htmlFor={id} className="text-xs font-medium text-muted-foreground">
        {label}
      </Label>
      {children}
      {hint ? (
        <p className="text-[11px] text-muted-foreground">{hint}</p>
      ) : null}
    </div>
  );
}

interface TextFieldProps {
  id?: string;
  label: string;
  value: string | null | undefined;
  onCommit: (next: string | null) => void;
  placeholder?: string;
  hint?: ReactNode;
  disabled?: boolean;
  className?: string;
}

export function TextField({
  id,
  label,
  value,
  onCommit,
  placeholder,
  hint,
  disabled,
  className,
}: TextFieldProps): JSX.Element {
  const [draft, setDraft] = useState(value ?? "");
  useEffect(() => setDraft(value ?? ""), [value]);
  return (
    <FieldShell id={id} label={label} hint={hint} className={className}>
      <Input
        id={id}
        value={draft}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          const trimmed = draft.trim();
          const next = trimmed.length === 0 ? null : draft;
          if ((value ?? null) === next) return;
          onCommit(next);
        }}
      />
    </FieldShell>
  );
}

interface NumberFieldProps {
  id?: string;
  label: string;
  /** Stored value (cents for prices, percent for GP, raw int for counts). */
  value: number | null | undefined;
  onCommit: (next: number | null) => void;
  /** Display divisor — set to 100 for cents-as-dollars, etc. */
  scale?: number;
  step?: number;
  prefix?: string;
  suffix?: string;
  placeholder?: string;
  hint?: ReactNode;
  disabled?: boolean;
}

export function NumberField({
  id,
  label,
  value,
  onCommit,
  scale = 1,
  step,
  prefix,
  suffix,
  placeholder,
  hint,
  disabled,
}: NumberFieldProps): JSX.Element {
  const toDisplay = (raw: number | null | undefined): string =>
    raw === null || raw === undefined ? "" : String(raw / scale);
  const [draft, setDraft] = useState(toDisplay(value));
  useEffect(() => setDraft(toDisplay(value)), [value, scale]);
  return (
    <FieldShell id={id} label={label} hint={hint}>
      <div className="relative">
        {prefix ? (
          <span className="pointer-events-none absolute inset-y-0 left-2 flex items-center text-xs text-muted-foreground">
            {prefix}
          </span>
        ) : null}
        <Input
          id={id}
          type="number"
          inputMode="decimal"
          step={step}
          value={draft}
          placeholder={placeholder}
          disabled={disabled}
          className={cn(prefix && "pl-7", suffix && "pr-8")}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => {
            if (draft.trim() === "") {
              if (value !== null && value !== undefined) onCommit(null);
              return;
            }
            const n = Number(draft);
            if (Number.isNaN(n)) {
              setDraft(toDisplay(value));
              return;
            }
            const raw = Math.round(n * scale);
            if (raw === value) return;
            onCommit(raw);
          }}
        />
        {suffix ? (
          <span className="pointer-events-none absolute inset-y-0 right-2 flex items-center text-xs text-muted-foreground">
            {suffix}
          </span>
        ) : null}
      </div>
    </FieldShell>
  );
}

interface DateFieldProps {
  id?: string;
  label: string;
  value: string | null | undefined;
  onCommit: (next: string | null) => void;
  hint?: ReactNode;
  disabled?: boolean;
}

export function DateField({
  id,
  label,
  value,
  onCommit,
  hint,
  disabled,
}: DateFieldProps): JSX.Element {
  return (
    <FieldShell id={id} label={label} hint={hint}>
      <DateInput
        id={id}
        value={value ?? ""}
        disabled={disabled}
        onChange={(e) => {
          const next = e.target.value || null;
          if ((value ?? null) === next) return;
          onCommit(next);
        }}
      />
    </FieldShell>
  );
}

interface PlainTextareaFieldProps {
  id?: string;
  label: string;
  value: string | null | undefined;
  onCommit: (next: string | null) => void;
  placeholder?: string;
  hint?: ReactNode;
  rows?: number;
  disabled?: boolean;
}

export function PlainTextareaField({
  id,
  label,
  value,
  onCommit,
  placeholder,
  hint,
  rows = 4,
  disabled,
}: PlainTextareaFieldProps): JSX.Element {
  const [draft, setDraft] = useState(value ?? "");
  useEffect(() => setDraft(value ?? ""), [value]);
  return (
    <FieldShell id={id} label={label} hint={hint}>
      <Textarea
        id={id}
        rows={rows}
        value={draft}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          const trimmed = draft.trim();
          const next = trimmed.length === 0 ? null : draft;
          if ((value ?? null) === next) return;
          onCommit(next);
        }}
      />
    </FieldShell>
  );
}

interface MarkdownFieldProps {
  label: string;
  value: string | null | undefined;
  onCommit: (next: string | null) => void;
  hint?: ReactNode;
  height?: number;
  disabled?: boolean;
}

/**
 * Markdown long-text field. Edits are committed on blur of the
 * editor container (so a quick switch to another tab still flushes
 * via the parent's autosave hook).
 */
export function MarkdownField({
  label,
  value,
  onCommit,
  hint,
  height = 240,
  disabled,
}: MarkdownFieldProps): JSX.Element {
  const { theme } = useTheme();
  const colorMode =
    theme === "dark" ||
    (theme === "system" &&
      typeof document !== "undefined" &&
      document.documentElement.classList.contains("dark"))
      ? "dark"
      : "light";
  const [draft, setDraft] = useState(value ?? "");
  useEffect(() => setDraft(value ?? ""), [value]);
  return (
    <FieldShell label={label} hint={hint}>
      <div
        data-color-mode={colorMode}
        onBlur={(e) => {
          // Only commit if focus is leaving the editor entirely —
          // toolbar buttons trigger blur on the textarea but keep
          // focus inside the container.
          if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
          const trimmed = draft.trim();
          const next = trimmed.length === 0 ? null : draft;
          if ((value ?? null) === next) return;
          onCommit(next);
        }}
      >
        <MDEditor
          value={draft}
          onChange={(v) => setDraft(v ?? "")}
          preview="edit"
          height={height}
          visibleDragbar={false}
          textareaProps={{ disabled }}
        />
      </div>
    </FieldShell>
  );
}

interface CheckboxGroupProps {
  label: string;
  options: readonly string[];
  value: readonly string[];
  onCommit: (next: string[]) => void;
  hint?: ReactNode;
  disabled?: boolean;
}

export function CheckboxGroup({
  label,
  options,
  value,
  onCommit,
  hint,
  disabled,
}: CheckboxGroupProps): JSX.Element {
  const selected = new Set(value);
  const toggle = (opt: string): void => {
    const next = new Set(selected);
    if (next.has(opt)) next.delete(opt);
    else next.add(opt);
    onCommit([...next]);
  };
  return (
    <FieldShell label={label} hint={hint}>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        {options.map((opt) => {
          const id = `cbg-${label}-${opt}`.replace(/[^a-zA-Z0-9-]/g, "-");
          const checked = selected.has(opt);
          return (
            <label
              key={opt}
              htmlFor={id}
              className={cn(
                "flex cursor-pointer items-center gap-2 rounded-md border px-2.5 py-1.5 text-xs",
                checked
                  ? "border-primary bg-primary/5"
                  : "border-border bg-background hover:bg-accent/30",
                disabled && "pointer-events-none opacity-60",
              )}
            >
              <Checkbox
                id={id}
                checked={checked}
                disabled={disabled}
                onCheckedChange={() => toggle(opt)}
              />
              <span className="truncate">{opt}</span>
            </label>
          );
        })}
      </div>
    </FieldShell>
  );
}
