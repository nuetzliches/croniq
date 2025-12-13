import { z } from 'zod';

const scopesSchema = z.array(z.string()).optional().nullable();

export const issueApiKeyRequestSchema = z.object({
    clientId: z.string().min(1),
    environmentTag: z.string().optional().nullable(),
    scopes: scopesSchema,
    ttlHours: z.number().int().optional().nullable(),
});

export type IssueApiKeyRequest = z.infer<typeof issueApiKeyRequestSchema>;
