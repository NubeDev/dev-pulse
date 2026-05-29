import { useRef, useState } from "react";
import { ImageIcon, TrashIcon, UploadCloudIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { cn } from "@/lib/utils";

import type { ExecSummaryDto } from "../../../api/client.js";
import {
  useDeleteExecSummaryImage,
  useExecSummaryAutosave,
  useUploadExecSummaryImage,
} from "../hooks/use-exec-summary.js";
import { MarkdownField, TextField } from "../form-fields.js";

export function HardwareSection({
  projectId,
  data,
}: {
  projectId: string;
  data: ExecSummaryDto;
}): JSX.Element {
  const { patch } = useExecSummaryAutosave(projectId);
  const h = data.hardware;
  const upload = useUploadExecSummaryImage(projectId);
  const remove = useDeleteExecSummaryImage(projectId);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [dragOver, setDragOver] = useState(false);

  const handleFiles = (files: FileList | null): void => {
    if (!files) return;
    for (const file of Array.from(files)) {
      if (!file.type.startsWith("image/")) continue;
      upload.mutate({ file });
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <div data-validation-key="hardware.hardware_features">
        <MarkdownField
          label="Hardware features"
          value={h.hardware_features}
          onCommit={(hardware_features) =>
            patch({ hardware: { hardware_features } })
          }
        />
      </div>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        <TextField
          id="es-enclosure"
          label="Enclosure"
          value={h.enclosure}
          onCommit={(enclosure) => patch({ hardware: { enclosure } })}
          placeholder="ABS, IP54"
        />
        <TextField
          id="es-mounting-type"
          label="Mounting type"
          value={h.mounting_type}
          onCommit={(mounting_type) => patch({ hardware: { mounting_type } })}
        />
        <TextField
          id="es-env"
          label="Operating environment"
          value={h.operating_env}
          onCommit={(operating_env) =>
            patch({ hardware: { operating_env } })
          }
          placeholder="0–50 °C, 10–90% RH"
        />
      </div>
      <MarkdownField
        label="Physical notes"
        value={h.physical_notes}
        onCommit={(physical_notes) =>
          patch({ hardware: { physical_notes } })
        }
      />

      <div className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-medium">Reference images</h3>
          <span className="text-xs text-muted-foreground">
            {data.images.length} attached
          </span>
        </div>
        <div
          className={cn(
            "flex flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed px-4 py-8 text-center transition-colors",
            dragOver
              ? "border-primary bg-primary/5"
              : "border-border bg-muted/20",
          )}
          onDragOver={(e) => {
            e.preventDefault();
            setDragOver(true);
          }}
          onDragLeave={() => setDragOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            setDragOver(false);
            handleFiles(e.dataTransfer.files);
          }}
        >
          <UploadCloudIcon className="h-6 w-6 text-muted-foreground" />
          <p className="text-sm text-muted-foreground">
            Drop reference images here, or
          </p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={upload.isPending}
            onClick={() => inputRef.current?.click()}
          >
            {upload.isPending ? "Uploading…" : "Browse files"}
          </Button>
          <input
            ref={inputRef}
            type="file"
            accept="image/*"
            multiple
            className="hidden"
            onChange={(e) => {
              handleFiles(e.target.files);
              e.target.value = "";
            }}
          />
        </div>
        {upload.isError && (
          <Alert variant="destructive">
            <AlertTitle>Upload failed</AlertTitle>
            <AlertDescription>{upload.error.message}</AlertDescription>
          </Alert>
        )}
        {data.images.length > 0 && (
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {data.images.map((img) => (
              <figure
                key={img.id}
                className="group relative overflow-hidden rounded-md border bg-muted"
              >
                <img
                  src={img.url}
                  alt={img.caption ?? img.filename}
                  className="aspect-video w-full object-cover"
                  onError={(e) => {
                    // Graceful fallback if the proxy URL is missing /
                    // unreachable — show the icon placeholder rather
                    // than a broken-image glyph.
                    e.currentTarget.style.display = "none";
                  }}
                />
                <figcaption className="flex items-center justify-between gap-2 px-2 py-1.5 text-[11px]">
                  <span className="flex min-w-0 items-center gap-1.5">
                    <ImageIcon className="h-3 w-3 shrink-0 text-muted-foreground" />
                    <span className="truncate">
                      {img.caption ?? img.filename}
                    </span>
                  </span>
                  <button
                    type="button"
                    className="text-muted-foreground hover:text-destructive"
                    title="Remove image"
                    disabled={remove.isPending}
                    onClick={() => remove.mutate(img.id)}
                  >
                    <TrashIcon className="h-3.5 w-3.5" />
                  </button>
                </figcaption>
              </figure>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
