import { z } from 'zod';
import { CreateWebhookIpRuleRequest, RotateWebhookSecretRequest, UpsertWebhookEndpointRequest } from '../generated/schemas';

export const createWebhookIpRuleRequestSchema = CreateWebhookIpRuleRequest;
export const rotateWebhookSecretRequestSchema = RotateWebhookSecretRequest;
export const upsertWebhookEndpointRequestSchema = UpsertWebhookEndpointRequest;

export const webhookActivityStatusSchema = z.enum(['success', 'failed', 'warning']);

export const WebhookActivityTimelineEntry = z.object({
    id: z.string().optional(),
    kind: z.enum(['delivery', 'deadLetter']).optional(),
    status: webhookActivityStatusSchema.optional(),
    hookKey: z.string().optional(),
    jobKey: z.string().nullable().optional(),
    environment: z.string().nullable().optional(),
    occurredAtUtc: z.iso.datetime({ offset: true }).optional(),
    latencyMs: z.number().int().nonnegative().nullable().optional(),
    payloadBytes: z.number().int().nonnegative().nullable().optional(),
    requestId: z.string().nullable().optional(),
    reason: z.string().nullable().optional(),
    deadLetterId: z.number().int().nullable().optional(),
});

export const WebhookActivityTimelineResponse = z.array(WebhookActivityTimelineEntry);

export const WebhookActivityBucket = z.object({
    bucketStartUtc: z.iso.datetime({ offset: true }).optional(),
    bucketEndUtc: z.iso.datetime({ offset: true }).nullable().optional(),
    totalCount: z.number().int().nonnegative().optional(),
    errorCount: z.number().int().nonnegative().optional(),
    p95LatencyMs: z.number().int().nonnegative().nullable().optional(),
});

export const WebhookActivitySummary = z.object({
    bucketMinutes: z.number().int().positive().nullable().optional(),
    windowStartUtc: z.iso.datetime({ offset: true }).nullable().optional(),
    windowEndUtc: z.iso.datetime({ offset: true }).nullable().optional(),
    buckets: z.array(WebhookActivityBucket).default([]),
});

export type WebhookActivityStatus = z.infer<typeof webhookActivityStatusSchema>;
export type WebhookActivityTimelineEntry = z.infer<typeof WebhookActivityTimelineEntry>;
export type WebhookActivityTimelineResponse = z.infer<typeof WebhookActivityTimelineResponse>;
export type WebhookActivityBucket = z.infer<typeof WebhookActivityBucket>;
export type WebhookActivitySummary = z.infer<typeof WebhookActivitySummary>;

