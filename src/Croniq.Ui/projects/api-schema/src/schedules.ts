import { z } from 'zod';

export const scheduleStateSchema = z.enum(['active', 'paused', 'degraded']);

export const scheduleSummarySchema = z.object({
    id: z.string().uuid(),
    name: z.string().min(1),
    tenant: z.string().min(1),
    cron: z.string().min(1),
    timezone: z.string().min(1),
    owner: z.string().min(1),
    state: scheduleStateSchema,
    nextFire: z.string().datetime(),
    lastDurationMs: z.number().nonnegative(),
    alerts: z.number().int().nonnegative(),
    tags: z.array(z.string()).default([]),
});

export const scheduleListResponseSchema = z.object({
    items: z.array(scheduleSummarySchema),
    total: z.number().int().nonnegative(),
    updatedAt: z.string().datetime(),
});

const metadataRecordSchema = z
    .record(z.string(), z.string())
    .optional()
    .nullable();

export const upsertScheduleRequestSchema = z.object({
    jobKey: z.string().min(1),
    cronExpression: z.string().min(1),
    triggerId: z.string().min(1).optional().nullable(),
    startAtUtc: z.string().datetime().optional().nullable(),
    endAtUtc: z.string().datetime().optional().nullable(),
    enabled: z.boolean().optional(),
    description: z.string().optional().nullable(),
    metadata: metadataRecordSchema,
});

export type ScheduleState = z.infer<typeof scheduleStateSchema>;
export type ScheduleSummary = z.infer<typeof scheduleSummarySchema>;
export type ScheduleListResponse = z.infer<typeof scheduleListResponseSchema>;
export type UpsertScheduleRequest = z.infer<typeof upsertScheduleRequestSchema>;
