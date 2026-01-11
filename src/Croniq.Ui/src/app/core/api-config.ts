import { z } from 'zod';

const urlLikeSchema = z
    .string()
    .trim()
    .min(1)
    .refine((value) => value.startsWith('/') || value.startsWith('http://') || value.startsWith('https://'), {
        message: 'Expected an absolute URL (http/https) or an absolute path starting with /',
    });
const nonEmptyStringSchema = z.string().trim().min(1);

export const croniqUiRuntimeConfigSchema = z
    .object({
        apiBaseUrl: urlLikeSchema.optional(),
        swaggerUiUrl: urlLikeSchema.optional(),
        grafanaUrl: urlLikeSchema.optional(),
        defaultTenantId: nonEmptyStringSchema.optional(),
    })
    .strict();

export type CroniqUiRuntimeConfig = z.infer<typeof croniqUiRuntimeConfigSchema>;

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
