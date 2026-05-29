/**
 * Data hooks for the project Executive Summary surface
 * (SCOPE-PROJECT-EXECUTIVE-SUMMARY.md §4).
 *
 * Single source of truth: one react-query cache entry keyed by
 * `["exec-summary", projectId]`. All mutations (section patches,
 * approval transitions, file/changelog add/remove) invalidate that
 * entry so the header completion bar and section state stay
 * coherent without per-mutation hand-merging.
 */

import { useCallback, useEffect, useRef } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from "@tanstack/react-query";

import { api } from "../../../api/client.js";
import type {
  AddChangelogEntryRequest,
  ApproveExecSummaryRequest,
  ExecSummaryChangelogEntry,
  ExecSummaryDocumentDto,
  ExecSummaryDto,
  ExecSummaryImageDto,
  PatchExecSummaryRequest,
} from "../../../api/client.js";

const execSummaryKey = (projectId: string): readonly unknown[] => [
  "exec-summary",
  projectId,
];

export function useExecSummary(
  projectId: string,
): UseQueryResult<ExecSummaryDto> {
  return useQuery({
    queryKey: execSummaryKey(projectId),
    queryFn: () => api.getProjectExecSummary(projectId),
    staleTime: 5_000,
  });
}

export function usePatchExecSummary(
  projectId: string,
): UseMutationResult<ExecSummaryDto, Error, PatchExecSummaryRequest> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body) => api.patchProjectExecSummary(projectId, body),
    onSuccess: (next) => {
      qc.setQueryData(execSummaryKey(projectId), next);
    },
  });
}

/**
 * Debounced, section-scoped auto-save.
 *
 * Holds one pending patch in a ref and flushes it after `delay` ms
 * of inactivity. `flush()` lets the caller force a write on tab
 * switch or unmount so edits are never silently dropped.
 */
export function useExecSummaryAutosave(
  projectId: string,
  delay = 800,
): {
  patch: (body: PatchExecSummaryRequest) => void;
  flush: () => void;
  isPending: boolean;
  error: Error | null;
} {
  const mutation = usePatchExecSummary(projectId);
  const pendingRef = useRef<PatchExecSummaryRequest | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const flush = useCallback((): void => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    const body = pendingRef.current;
    pendingRef.current = null;
    if (body) mutation.mutate(body);
  }, [mutation]);

  const patch = useCallback(
    (body: PatchExecSummaryRequest): void => {
      pendingRef.current = mergePatch(pendingRef.current, body);
      if (timerRef.current) clearTimeout(timerRef.current);
      timerRef.current = setTimeout(flush, delay);
    },
    [delay, flush],
  );

  useEffect(() => {
    return () => {
      // Flush-on-unmount: don't drop the user's last keystroke
      // when they navigate away (spec §4.3).
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        flush();
      }
    };
  }, [flush]);

  return {
    patch,
    flush,
    isPending: mutation.isPending,
    error: mutation.error,
  };
}

/** Shallow-merge the per-section patch payloads so a fast
 *  sequence of edits ends up as one PATCH rather than several. */
function mergePatch(
  a: PatchExecSummaryRequest | null,
  b: PatchExecSummaryRequest,
): PatchExecSummaryRequest {
  if (!a) return b;
  // Strongly-typed shallow merge across the section keys would
  // require a 6-way overload set; the per-section payloads are
  // plain object literals so a single cast at the end is the
  // smaller hammer.
  const merged = { ...a } as Record<string, unknown>;
  for (const [key, bv] of Object.entries(b)) {
    const av = (a as Record<string, unknown>)[key];
    if (av && bv && typeof av === "object" && typeof bv === "object") {
      merged[key] = { ...(av as object), ...(bv as object) };
    } else if (bv !== undefined) {
      merged[key] = bv;
    }
  }
  return merged as PatchExecSummaryRequest;
}

// ---------------------------------------------------------------------------
// State-machine actions
// ---------------------------------------------------------------------------

export function useSubmitExecSummary(
  projectId: string,
): UseMutationResult<ExecSummaryDto, Error, { force?: boolean } | void> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (vars) =>
      api.submitProjectExecSummary(projectId, {
        force: vars?.force === true,
      }),
    onSuccess: (next) => qc.setQueryData(execSummaryKey(projectId), next),
  });
}

export function useApproveExecSummary(
  projectId: string,
): UseMutationResult<ExecSummaryDto, Error, ApproveExecSummaryRequest | void> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body) =>
      api.approveProjectExecSummary(projectId, body ?? {}),
    onSuccess: (next) => qc.setQueryData(execSummaryKey(projectId), next),
  });
}

export function useRevertExecSummary(
  projectId: string,
): UseMutationResult<ExecSummaryDto, Error, void> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api.revertProjectExecSummary(projectId),
    onSuccess: (next) => qc.setQueryData(execSummaryKey(projectId), next),
  });
}

// ---------------------------------------------------------------------------
// File uploads + changelog
// ---------------------------------------------------------------------------

/**
 * Inline-image uploader for markdown editors inside the exec
 * summary. Returns a single async function the editor calls with a
 * dropped/pasted `File`; resolves to the proxy URL that should be
 * inserted into the markdown body. Reuses the project's reference-
 * image endpoint so every embedded image is stored, audited, and
 * deleted alongside the rest of the summary.
 */
export function useExecSummaryInlineImageUploader(
  projectId: string,
): (file: File) => Promise<string> {
  return useCallback(
    async (file) => {
      const dto = await api.uploadProjectExecSummaryImage(projectId, file);
      return dto.url;
    },
    [projectId],
  );
}

export function useUploadExecSummaryImage(
  projectId: string,
): UseMutationResult<ExecSummaryImageDto, Error, { file: File; caption?: string }> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ file, caption }) =>
      api.uploadProjectExecSummaryImage(projectId, file, caption),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: execSummaryKey(projectId) });
    },
  });
}

export function useDeleteExecSummaryImage(
  projectId: string,
): UseMutationResult<void, Error, string> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (imageId) =>
      api.deleteProjectExecSummaryImage(projectId, imageId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: execSummaryKey(projectId) });
    },
  });
}

export interface UploadDocumentVars {
  file: File;
  title: string;
  doc_type?: string;
  notes?: string;
  required_action?: string;
}

export function useUploadExecSummaryDocument(
  projectId: string,
): UseMutationResult<ExecSummaryDocumentDto, Error, UploadDocumentVars> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ file, ...rest }) =>
      api.uploadProjectExecSummaryDocument(projectId, file, rest),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: execSummaryKey(projectId) });
    },
  });
}

export function usePatchExecSummaryDocument(
  projectId: string,
): UseMutationResult<
  ExecSummaryDocumentDto,
  Error,
  {
    documentId: string;
    body: {
      title?: string;
      doc_type?: string | null;
      notes?: string | null;
      required_action?: string | null;
    };
  }
> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ documentId, body }) =>
      api.patchProjectExecSummaryDocument(projectId, documentId, body),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: execSummaryKey(projectId) });
    },
  });
}

export function useDeleteExecSummaryDocument(
  projectId: string,
): UseMutationResult<void, Error, string> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (documentId) =>
      api.deleteProjectExecSummaryDocument(projectId, documentId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: execSummaryKey(projectId) });
    },
  });
}

export function useAddExecSummaryChangelog(
  projectId: string,
): UseMutationResult<ExecSummaryChangelogEntry, Error, AddChangelogEntryRequest> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body) => api.addProjectExecSummaryChangelog(projectId, body),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: execSummaryKey(projectId) });
    },
  });
}

export function useDeleteExecSummaryChangelog(
  projectId: string,
): UseMutationResult<void, Error, string> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (entryId) =>
      api.deleteProjectExecSummaryChangelog(projectId, entryId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: execSummaryKey(projectId) });
    },
  });
}
