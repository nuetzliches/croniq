import { z } from 'zod';
export const UpsertScheduleRequest = z.object({
    jobKey: z.string().min(1),
    cronExpression: z.string().min(1),
    triggerId: z.string().nullish(),
    startAtUtc: z.string().datetime({ offset: true }).nullish(),
    endAtUtc: z.string().datetime({ offset: true }).nullish(),
    enabled: z.boolean().optional(),
    description: z.string().nullish(),
    metadata: z.record(z.string(), z.string()).nullish(),
});
export type UpsertScheduleRequest = z.infer<typeof UpsertScheduleRequest>;
export const UpsertWebhookEndpointRequest = z.object({
    hookKey: z.string().min(1),
    jobKey: z.string().min(1),
    enabled: z.boolean().optional(),
    requireSignature: z.boolean().optional(),
    requestsPerMinute: z.number().int().nullish(),
    secret: z.string().nullish(),
    metadata: z.record(z.string(), z.string()).nullish(),
    signatureVersion: z.number().int().optional(),
});
export type UpsertWebhookEndpointRequest = z.infer<
    typeof UpsertWebhookEndpointRequest
>;
export const RotateWebhookSecretRequest = z
    .object({
        activateInSeconds: z.number().int().nullable(),
        gracePeriodSeconds: z.number().int().nullable(),
        notes: z.string().nullable(),
    })
    .partial();
export type RotateWebhookSecretRequest = z.infer<
    typeof RotateWebhookSecretRequest
>;
export const CreateWebhookIpRuleRequest = z.object({
    cidr: z.string().min(1),
    description: z.string().nullish(),
});
export type CreateWebhookIpRuleRequest = z.infer<
    typeof CreateWebhookIpRuleRequest
>;
export const IssueApiKeyRequest = z.object({
    clientId: z.string().min(1),
    environmentTag: z.string().nullish(),
    scopes: z.array(z.string()).nullish(),
    ttlHours: z.number().int().nullish(),
});
export type IssueApiKeyRequest = z.infer<typeof IssueApiKeyRequest>;
export const TriggerJobRequest = z.object({
    jobKey: z.string().min(1),
    metadata: z.record(z.string(), z.string()).nullish(),
});
export type TriggerJobRequest = z.infer<typeof TriggerJobRequest>;
export const schemas = {
    UpsertScheduleRequest,
    UpsertWebhookEndpointRequest,
    RotateWebhookSecretRequest,
    CreateWebhookIpRuleRequest,
    IssueApiKeyRequest,
    TriggerJobRequest,
};
export type HttpMethod =
    | 'get'
    | 'post'
    | 'put'
    | 'patch'
    | 'delete'
    | 'options'
    | 'head';
export type RequestFormat =
    | 'json'
    | 'binary'
    | 'form-data'
    | 'url-encoded'
    | 'multipart'
    | 'unknown';
export type ParameterLocation = 'Path' | 'Query' | 'Body' | 'Header';
export interface EndpointParameter<TSchema = unknown> {
    name: string;
    description?: string;
    type?: ParameterLocation;
    schema: TSchema;
}
export interface EndpointError<TSchema = unknown> {
    status: number | 'default';
    description?: string;
    schema: TSchema;
}
export interface EndpointDefinition<TResponse = unknown> {
    method: HttpMethod;
    path: string;
    description?: string;
    alias?: string;
    requestFormat?: RequestFormat;
    parameters?: EndpointParameter[];
    response: TResponse;
    errors?: EndpointError[];
}
export type EndpointList = ReadonlyArray<EndpointDefinition>;
