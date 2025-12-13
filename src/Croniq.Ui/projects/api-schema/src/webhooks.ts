import { z } from 'zod';

const metadataRecordSchema = z
    .record(z.string(), z.string())
    .optional()
    .nullable();

export const createWebhookIpRuleRequestSchema = z.object({
    cidr: z.string().min(1),
    description: z.string().optional().nullable(),
});

export const rotateWebhookSecretRequestSchema = z.object({
    activateInSeconds: z.number().int().optional().nullable(),
    gracePeriodSeconds: z.number().int().optional().nullable(),
    notes: z.string().optional().nullable(),
});

export const upsertWebhookEndpointRequestSchema = z.object({
    hookKey: z.string().min(1),
    jobKey: z.string().min(1),
    enabled: z.boolean().optional(),
    requireSignature: z.boolean().optional(),
    requestsPerMinute: z.number().int().optional().nullable(),
    secret: z.string().optional().nullable(),
    metadata: metadataRecordSchema,
    signatureVersion: z.number().int().optional(),
});

export type CreateWebhookIpRuleRequest = z.infer<typeof createWebhookIpRuleRequestSchema>;
export type RotateWebhookSecretRequest = z.infer<typeof rotateWebhookSecretRequestSchema>;
export type UpsertWebhookEndpointRequest = z.infer<typeof upsertWebhookEndpointRequestSchema>;
