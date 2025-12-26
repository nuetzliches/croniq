import { z } from 'zod';
export const UpsertTenantRequest = z.object({
    tenantId: z.string().min(1),
    name: z.string().min(1),
});
export type UpsertTenantRequest = z.infer<typeof UpsertTenantRequest>;
export const UpsertJobRequest = z.object({
    jobKey: z.string().min(1),
    namespace: z.string().min(1),
    name: z.string().min(1),
    variant: z.string().nullish(),
    description: z.string().nullish(),
    metadata: z.record(z.string(), z.string()).nullish(),
});
export type UpsertJobRequest = z.infer<typeof UpsertJobRequest>;
export const CroniqTriggerSeedDefinition = z
    .object({
        triggerId: z.string().nullable(),
        jobKey: z.string().nullable(),
        cronExpression: z.string().nullable(),
        startAtUtc: z.iso.datetime({ offset: true }).nullable(),
        endAtUtc: z.iso.datetime({ offset: true }).nullable(),
        enabled: z.boolean(),
        metadata: z.record(z.string(), z.string()).nullable(),
        description: z.string().nullable(),
        managedBy: z.string().nullable(),
        timeZoneId: z.string().nullable(),
    })
    .partial();
export type CroniqTriggerSeedDefinition = z.infer<
    typeof CroniqTriggerSeedDefinition
>;
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
export const UpsertApiClientRequest = z.object({
    clientId: z.string().min(1),
    name: z.string().nullish(),
    environmentTag: z.string().nullish(),
    scopes: z.array(z.string()).nullish(),
    isActive: z.boolean().nullish(),
});
export type UpsertApiClientRequest = z.infer<typeof UpsertApiClientRequest>;
export const IssueApiKeyRequest = z.object({
    clientId: z.string().min(1),
    environmentTag: z.string().nullish(),
    scopes: z.array(z.string()).nullish(),
    ttlHours: z.number().int().nullish(),
});
export type IssueApiKeyRequest = z.infer<typeof IssueApiKeyRequest>;
export const IssueTokenRequest = z
    .object({
        clientId: z.string().nullable(),
        scopes: z.array(z.string()).nullable(),
        audience: z.string().nullable(),
        ttlMinutes: z.number().int().nullable(),
    })
    .partial();
export type IssueTokenRequest = z.infer<typeof IssueTokenRequest>;
export const PasswordLoginRequest = z
    .object({
        username: z.string().nullable(),
        password: z.string().nullable(),
        tenantId: z.string().nullable(),
        environmentTag: z.string().nullable(),
        scopes: z.array(z.string()).nullable(),
        audience: z.string().nullable(),
    })
    .partial();
export type PasswordLoginRequest = z.infer<typeof PasswordLoginRequest>;
export const PasswordRefreshRequest = z
    .object({
        refreshToken: z.string().nullable(),
        tenantId: z.string().nullable(),
        environmentTag: z.string().nullable(),
        scopes: z.array(z.string()).nullable(),
        audience: z.string().nullable(),
    })
    .partial();
export type PasswordRefreshRequest = z.infer<typeof PasswordRefreshRequest>;
export const PasswordLogoutRequest = z
    .object({
        refreshToken: z.string().nullable(),
        tenantId: z.string().nullable(),
    })
    .partial();
export type PasswordLogoutRequest = z.infer<typeof PasswordLogoutRequest>;
export const PasswordChangePasswordRequest = z
    .object({
        currentPassword: z.string().nullable(),
        newPassword: z.string().nullable(),
    })
    .partial();
export type PasswordChangePasswordRequest = z.infer<
    typeof PasswordChangePasswordRequest
>;
export const TriggerJobRequest = z.object({
    jobKey: z.string().min(1),
    metadata: z.record(z.string(), z.string()).nullish(),
    delaySeconds: z.number().int().nullish(),
});
export type TriggerJobRequest = z.infer<typeof TriggerJobRequest>;
export const WorkPollRequest = z.object({
    environmentTag: z.string().nullish(),
    runnerId: z.string().min(1),
    batchSize: z.number().int().nullish(),
    waitForMs: z.number().int().nullish(),
});
export type WorkPollRequest = z.infer<typeof WorkPollRequest>;
export const WorkLeaseToken = z.object({
    executionId: z.string().min(1),
    leaseId: z.string().min(1),
    triggerId: z.string().min(1),
    jobKey: z.string().min(1),
    fireAtUtc: z.iso.datetime({ offset: true }).optional(),
    leaseExpiresAtUtc: z.iso.datetime({ offset: true }).optional(),
    payload: z.string().nullish(),
});
export type WorkLeaseToken = z.infer<typeof WorkLeaseToken>;
export const WorkRenewRequest = z.object({
    environmentTag: z.string().nullish(),
    runnerId: z.string().min(1),
    lease: WorkLeaseToken,
});
export type WorkRenewRequest = z.infer<typeof WorkRenewRequest>;
export const WorkAckRequest = z.object({
    environmentTag: z.string().nullish(),
    runnerId: z.string().min(1),
    lease: WorkLeaseToken,
    succeeded: z.boolean().optional(),
    nextFireTimeUtc: z.iso.datetime({ offset: true }).nullish(),
    deadLetterReason: z.string().nullish(),
});
export type WorkAckRequest = z.infer<typeof WorkAckRequest>;
export const WorkEventEntry = z.object({
    message: z.string().min(1),
    level: z.string().nullish(),
    timestampUtc: z.iso.datetime({ offset: true }).nullish(),
    properties: z.record(z.string(), z.string()).nullish(),
    eventType: z.string().nullish(),
});
export type WorkEventEntry = z.infer<typeof WorkEventEntry>;
export const WorkEventsRequest = z.object({
    environmentTag: z.string().nullish(),
    runnerId: z.string().min(1),
    lease: WorkLeaseToken,
    events: z.array(WorkEventEntry).nullish(),
});
export type WorkEventsRequest = z.infer<typeof WorkEventsRequest>;
export const RunnerHeartbeatRequest = z.object({
    environmentTag: z.string().nullish(),
    runnerId: z.string().min(1),
    seenAtUtc: z.iso.datetime({ offset: true }).nullish(),
    metadataJson: z.string().nullish(),
});
export type RunnerHeartbeatRequest = z.infer<typeof RunnerHeartbeatRequest>;
export const ExecutionStatus = z.union([
    z.literal(0),
    z.literal(1),
    z.literal(2),
]);
export type ExecutionStatus = z.infer<typeof ExecutionStatus>;
export const schemas = {
    UpsertTenantRequest,
    UpsertJobRequest,
    CroniqTriggerSeedDefinition,
    UpsertWebhookEndpointRequest,
    RotateWebhookSecretRequest,
    CreateWebhookIpRuleRequest,
    UpsertApiClientRequest,
    IssueApiKeyRequest,
    IssueTokenRequest,
    PasswordLoginRequest,
    PasswordRefreshRequest,
    PasswordLogoutRequest,
    PasswordChangePasswordRequest,
    TriggerJobRequest,
    WorkPollRequest,
    WorkLeaseToken,
    WorkRenewRequest,
    WorkAckRequest,
    WorkEventEntry,
    WorkEventsRequest,
    RunnerHeartbeatRequest,
    ExecutionStatus,
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
