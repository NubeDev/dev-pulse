import { z } from "zod";
import { isoDateTime, uuid } from "./common.js";

// ---------------------------------------------------------------------------
// Repos (directory + activity stats)
// ---------------------------------------------------------------------------

export const RepoSummaryDtoSchema = z.object({
  id: uuid,
  org_id: uuid,
  org_login: z.string(),
  name: z.string(),
  slug: z.string(),
  open_issue_count: z.number().int(),
  last_activity_at: isoDateTime.nullable().optional(),
});
export type RepoSummaryDto = z.infer<typeof RepoSummaryDtoSchema>;

export const RepoListResponseSchema = z.object({
  rows: z.array(RepoSummaryDtoSchema),
  total: z.number().int(),
  limit: z.number().int(),
  offset: z.number().int(),
});
export type RepoListResponse = z.infer<typeof RepoListResponseSchema>;

export interface ListReposQuery {
  org_id?: string;
  q?: string;
  limit?: number;
  offset?: number;
}

export const RepoMetadataDtoSchema = z.object({
  stars: z.number().int(),
  forks: z.number().int(),
  watchers: z.number().int(),
  open_issues_remote: z.number().int(),
  primary_language: z.string().nullable().optional(),
  default_branch: z.string().nullable().optional(),
  description: z.string().nullable().optional(),
  homepage: z.string().nullable().optional(),
  is_archived: z.boolean(),
  is_fork: z.boolean(),
  is_private: z.boolean(),
  pushed_at: isoDateTime.nullable().optional(),
  metadata_updated_at: isoDateTime,
});
export type RepoMetadataDto = z.infer<typeof RepoMetadataDtoSchema>;

export const PercentileTripleDtoSchema = z.object({
  p50: z.number().nullable().optional(),
  p90: z.number().nullable().optional(),
  p95: z.number().nullable().optional(),
});
export type PercentileTripleDto = z.infer<typeof PercentileTripleDtoSchema>;

export const RepoPrSizeStatsDtoSchema = z.object({
  since: isoDateTime,
  until: isoDateTime,
  sample_n: z.number().int(),
  additions: PercentileTripleDtoSchema,
  deletions: PercentileTripleDtoSchema,
  total_lines: PercentileTripleDtoSchema,
  changed_files: PercentileTripleDtoSchema,
  commits: PercentileTripleDtoSchema,
});
export type RepoPrSizeStatsDto = z.infer<typeof RepoPrSizeStatsDtoSchema>;

export const RepoCiStatsDtoSchema = z.object({
  since: isoDateTime,
  until: isoDateTime,
  total_runs: z.number().int(),
  success: z.number().int(),
  failure: z.number().int(),
  cancelled: z.number().int(),
  other: z.number().int(),
  success_rate: z.number().nullable().optional(),
  duration_sample_n: z.number().int(),
  duration_seconds: PercentileTripleDtoSchema,
});
export type RepoCiStatsDto = z.infer<typeof RepoCiStatsDtoSchema>;

export const HeatmapBucketDtoSchema = z.object({
  dow: z.number().int(),
  hour: z.number().int(),
  count: z.number().int(),
});
export type HeatmapBucketDto = z.infer<typeof HeatmapBucketDtoSchema>;

export const RepoActivityHeatmapDtoSchema = z.object({
  since: isoDateTime,
  until: isoDateTime,
  timezone: z.string(),
  total: z.number().int(),
  buckets: z.array(HeatmapBucketDtoSchema),
});
export type RepoActivityHeatmapDto = z.infer<typeof RepoActivityHeatmapDtoSchema>;

export const RepoReviewVelocityDtoSchema = z.object({
  since: isoDateTime,
  until: isoDateTime,
  sample_n: z.number().int(),
  time_to_merge_seconds: PercentileTripleDtoSchema,
});
export type RepoReviewVelocityDto = z.infer<typeof RepoReviewVelocityDtoSchema>;

export const RepoContributorDiversityDtoSchema = z.object({
  since: isoDateTime,
  until: isoDateTime,
  sample_n: z.number().int(),
  distinct_authors: z.number().int(),
  top1_share: z.number().nullable().optional(),
  top3_share: z.number().nullable().optional(),
});
export type RepoContributorDiversityDto = z.infer<typeof RepoContributorDiversityDtoSchema>;
