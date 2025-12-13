import { z } from 'zod';

const metadataRecordSchema = z
    .record(z.string(), z.string())
    .optional()
    .nullable();

export const triggerJobRequestSchema = z.object({
    jobKey: z.string().min(1),
    metadata: metadataRecordSchema,
});

export type TriggerJobRequest = z.infer<typeof triggerJobRequestSchema>;
