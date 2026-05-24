import { PrintableContent, usePrint } from "@nube/starter-ui-export";
import { useEffect, useMemo, useRef } from "react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

import type { ExecSummaryDto, ProjectDto } from "../../../api/client.js";

import { ExecSummaryPrintDocument } from "./exec-summary-print-document.js";

/**
 * Mounts the print document into a hidden host and fires the native
 * print dialog as soon as the host is ready. The browser's "Save as
 * PDF" option in the dialog produces the file; multi-page flow is
 * handled by `@page` rules injected by `printNode`.
 *
 * `onDone` fires on the `afterprint` event so the parent can unmount
 * us and let the next click start a fresh render.
 */
export function PrintHost({
  project,
  data,
  onDone,
}: {
  project: ProjectDto;
  data: ExecSummaryDto;
  onError: (e: Error) => void;
  onDone: () => void;
}): JSX.Element {
  const { hostRef, print, error } = usePrint();
  const fired = useRef(false);
  const generatedAt = useMemo(() => new Date(), []);

  // Trigger once the portal host is attached. `printNode` already
  // waits on fonts + image decode, so we don't need an extra delay.
  // `title` becomes the browser's print-chrome header AND the
  // default "Save as PDF" filename in Chromium.
  const wrappedHostRef = (node: HTMLDivElement | null) => {
    hostRef(node);
    if (node && !fired.current) {
      fired.current = true;
      const date = new Date().toISOString().slice(0, 10);
      const title = `${slugify(project.name)}-exec-summary-${date}`;
      void print({ title });
    }
  };

  useEffect(() => {
    const onAfter = () => onDone();
    window.addEventListener("afterprint", onAfter);
    return () => window.removeEventListener("afterprint", onAfter);
  }, [onDone]);

  return (
    <>
      <PrintableContent hostRef={wrappedHostRef}>
        <ExecSummaryPrintDocument
          project={project}
          data={data}
          generatedAt={generatedAt}
        />
      </PrintableContent>
      {error && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't generate PDF</AlertTitle>
          <AlertDescription>{error.message}</AlertDescription>
        </Alert>
      )}
    </>
  );
}

function slugify(s: string): string {
  return (
    s
      .normalize("NFKD")
      .replace(/\p{M}+/gu, "")
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 60) || "project"
  );
}
