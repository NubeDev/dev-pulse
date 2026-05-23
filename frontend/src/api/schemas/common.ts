import { z } from "zod";

/** RFC3339 instant string. */
export const isoDateTime = z.string().datetime({ offset: true });

/** UUID v4 string. */
export const uuid = z.string().uuid();

export const AckSchema = z.object({
  ok: z.boolean(),
});
export type Ack = z.infer<typeof AckSchema>;

export const CountRowSchema = z.object({
  key: z.string(),
  count: z.number().int(),
});
export type CountRow = z.infer<typeof CountRowSchema>;

export const HomeOrgSplitRowSchema = z.object({
  user_id: uuid,
  org_id: uuid,
  count: z.number().int(),
});
export type HomeOrgSplitRow = z.infer<typeof HomeOrgSplitRowSchema>;

export const DataAsOfSchema = z.object({
  headline: isoDateTime.nullable().optional(),
  per_org: z.record(uuid, isoDateTime),
  reconciler_latest: isoDateTime.nullable().optional(),
  webhook_latest: isoDateTime.nullable().optional(),
});
export type DataAsOf = z.infer<typeof DataAsOfSchema>;

export const ResolvedWindowSchema = z.object({
  start: isoDateTime,
  end: isoDateTime,
  label: z.string(),
  tz: z.string().optional(),
}).passthrough();
export type ResolvedWindow = z.infer<typeof ResolvedWindowSchema>;

export const ReportResponseSchema = z.object({
  resolved_window: ResolvedWindowSchema,
  data_as_of: DataAsOfSchema,
  rows: z.unknown(),
});
export type ReportResponse<TRow = unknown> = {
  resolved_window: ResolvedWindow;
  data_as_of: DataAsOf;
  rows: TRow;
};

export function reportResponseOf<TRow>(rowsSchema: z.ZodType<TRow>) {
  return z.object({
    resolved_window: ResolvedWindowSchema,
    data_as_of: DataAsOfSchema,
    rows: rowsSchema,
  });
}
