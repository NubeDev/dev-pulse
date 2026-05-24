import { PrinterIcon } from "lucide-react";
import { useState } from "react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";

import type { ExecSummaryDto } from "../../../api/client.js";
import { useProject } from "../../use-projects-data.js";

import { PrintHost } from "./print-host.js";

/**
 * Header button that triggers a browser-side print → "Save as PDF"
 * of the executive summary. The print host is only mounted while a
 * print is in flight.
 */
export function PrintPdfButton({
  projectId,
  data,
}: {
  projectId: string;
  data: ExecSummaryDto;
}): JSX.Element {
  const projectQuery = useProject(projectId);
  const [hostOpen, setHostOpen] = useState(false);

  const disabled = projectQuery.isPending || !projectQuery.data;

  return (
    <div className="flex flex-col items-end gap-1">
      <Button
        type="button"
        size="sm"
        variant="outline"
        disabled={disabled}
        onClick={() => setHostOpen(true)}
      >
        <PrinterIcon className="mr-1.5 h-3.5 w-3.5" />
        Download PDF
      </Button>

      {hostOpen && projectQuery.data && (
        <PrintHost
          project={projectQuery.data}
          data={data}
          onError={() => {
            // Stay mounted so the alert in PrintHost renders.
          }}
          onDone={() => setHostOpen(false)}
        />
      )}

      {projectQuery.isError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't load project</AlertTitle>
          <AlertDescription>{projectQuery.error.message}</AlertDescription>
        </Alert>
      )}
    </div>
  );
}
