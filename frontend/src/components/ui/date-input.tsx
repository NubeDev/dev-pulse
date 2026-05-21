import * as React from "react";
import { useRef, useState, useEffect } from "react";

import { cn } from "@/lib/utils";

/**
 * Date input that always shows dd/mm/yyyy format regardless of browser locale.
 * Includes a calendar icon that opens the native date picker.
 * Stores value as YYYY-MM-DD (ISO) internally, displays as dd/mm/yyyy.
 */
function DateInput({
  value,
  onChange,
  className,
  ...props
}: Omit<React.ComponentProps<"input">, "type" | "onChange" | "value"> & {
  value: string; // YYYY-MM-DD
  onChange: (e: { target: { value: string } }) => void;
}): JSX.Element {
  const [display, setDisplay] = useState("");
  const pickerRef = useRef<HTMLInputElement>(null);

  // Sync from external YYYY-MM-DD value → dd/mm/yyyy display
  useEffect(() => {
    if (!value) {
      setDisplay("");
      return;
    }
    const m = value.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (m) {
      setDisplay(`${m[3]}/${m[2]}/${m[1]}`);
    }
  }, [value]);

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>): void => {
    const raw = e.target.value;
    // Allow typing freely with digits and slashes
    const filtered = raw.replace(/[^\d/]/g, "").slice(0, 10);
    setDisplay(filtered);

    // Parse complete dd/mm/yyyy into YYYY-MM-DD
    const m = filtered.match(/^(\d{2})\/(\d{2})\/(\d{4})$/);
    if (m) {
      const [, dd, mm, yyyy] = m;
      const iso = `${yyyy}-${mm}-${dd}`;
      // Basic validity check
      const d = new Date(iso);
      if (!isNaN(d.getTime()) && d.toISOString().startsWith(iso)) {
        onChange({ target: { value: iso } });
        return;
      }
    }
    // If cleared, propagate empty
    if (filtered === "") {
      onChange({ target: { value: "" } });
    }
  };

  // Native picker changed — propagate
  const handlePickerChange = (e: React.ChangeEvent<HTMLInputElement>): void => {
    const iso = e.target.value; // YYYY-MM-DD from native picker
    if (iso) {
      onChange({ target: { value: iso } });
    } else {
      onChange({ target: { value: "" } });
    }
  };

  // Also support pasting ISO dates
  const handlePaste = (e: React.ClipboardEvent<HTMLInputElement>): void => {
    const text = e.clipboardData.getData("text").trim();
    const m = text.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (m) {
      e.preventDefault();
      setDisplay(`${m[3]}/${m[2]}/${m[1]}`);
      onChange({ target: { value: text } });
    }
  };

  return (
    <div className={cn("relative", className)}>
      <input
        {...props}
        type="text"
        inputMode="numeric"
        placeholder="dd/mm/yyyy"
        data-slot="input"
        className={cn(
          "h-9 w-full min-w-0 rounded-md border border-input bg-transparent px-3 py-1 pr-9 text-base shadow-xs transition-[color,box-shadow] outline-none selection:bg-primary selection:text-primary-foreground placeholder:text-muted-foreground disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm dark:bg-input/30",
          "focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50",
          "aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40",
        )}
        value={display}
        onChange={handleChange}
        onPaste={handlePaste}
      />
      {/* Hidden native date input used solely for its picker popup */}
      <input
        ref={pickerRef}
        type="date"
        value={value}
        onChange={handlePickerChange}
        className="absolute inset-0 opacity-0 pointer-events-none"
        tabIndex={-1}
        aria-hidden
      />
      {/* Calendar icon button to open native picker */}
      <button
        type="button"
        className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
        onClick={() => pickerRef.current?.showPicker()}
        tabIndex={-1}
        aria-label="Open date picker"
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 16 16"
          fill="currentColor"
          className="size-4"
        >
          <path
            fillRule="evenodd"
            d="M4 1.75a.75.75 0 0 1 1.5 0V3h5V1.75a.75.75 0 0 1 1.5 0V3a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2V1.75ZM4.5 7a1 1 0 0 0-1 1v4.5a.5.5 0 0 0 .5.5h8a.5.5 0 0 0 .5-.5V8a1 1 0 0 0-1-1h-7Z"
            clipRule="evenodd"
          />
        </svg>
      </button>
    </div>
  );
}

export { DateInput };
