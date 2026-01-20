import { z } from 'zod';

const urlLikeSchema = z
    .string()
    .trim()
    .min(1)
    .refine((value) => value.startsWith('/') || value.startsWith('http://') || value.startsWith('https://'), {
        message: 'Expected an absolute URL (http/https) or an absolute path starting with /',
    });
const nonEmptyStringSchema = z.string().trim().min(1);
const authModeSchema = z.enum(['password', 'oidc']);
const activityStreamModeSchema = z.enum(['grpc', 'sse', 'polling']);
const webhooksActivityStreamSchema = z
    .object({
        mode: activityStreamModeSchema.optional(),
        grpcBaseUrl: urlLikeSchema.optional(),
        sseBaseUrl: urlLikeSchema.optional(),
    })
    .strict();
const webhooksSchema = z
    .object({
        activityStream: webhooksActivityStreamSchema.optional(),
    })
    .strict();

export const croniqUiRuntimeConfigSchema = z
    .object({
        apiBaseUrl: urlLikeSchema.optional(),
        swaggerUiUrl: urlLikeSchema.optional(),
        grafanaUrl: urlLikeSchema.optional(),
        defaultTenantId: nonEmptyStringSchema.optional(),
        auth: z
            .object({
                mode: authModeSchema.optional(),
            })
            .strict()
            .optional(),
        webhooks: webhooksSchema.optional(),
    })
    .strict();

export type CroniqUiRuntimeConfig = z.infer<typeof croniqUiRuntimeConfigSchema>;
export type WebhookActivityStreamMode = z.infer<typeof activityStreamModeSchema>;
export type WebhookActivityStreamConfig = z.infer<typeof webhooksActivityStreamSchema>;

export function resolveSwaggerUiUrl(apiBaseUrl: string, explicitSwaggerUiUrl?: string | null): string {
    const trimmed = explicitSwaggerUiUrl?.trim();
    if (trimmed) {
        return trimmed;
    }
    if (!apiBaseUrl || apiBaseUrl.startsWith('/')) {
        return '/swagger/index.html';
    }
    return new URL('/swagger/index.html', apiBaseUrl).toString();
}
