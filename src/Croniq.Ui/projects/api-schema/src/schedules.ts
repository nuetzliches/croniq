import { z } from 'zod';
import { CroniqTriggerSeedDefinition } from '../generated/schemas';

export const scheduleStateSchema = z.enum(['active', 'paused', 'degraded']);

export const scheduleSummarySchema = z.object({
    id: z.string().uuid(),
    name: z.string().min(1),
    tenant: z.string().min(1),
    cron: z.string().min(1),
    timezone: z.string().min(1),
    owner: z.string().min(1),
    state: scheduleStateSchema,
    nextFire: z.iso.datetime(),
    lastDurationMs: z.number().nonnegative(),
    alerts: z.number().int().nonnegative(),
    tags: z.array(z.string()).default([]),
});

export const upsertScheduleRequestSchema = CroniqTriggerSeedDefinition;

export type ScheduleState = z.infer<typeof scheduleStateSchema>;
export type ScheduleSummary = z.infer<typeof scheduleSummarySchema>;
export type UpsertScheduleRequest = z.infer<typeof upsertScheduleRequestSchema>;
