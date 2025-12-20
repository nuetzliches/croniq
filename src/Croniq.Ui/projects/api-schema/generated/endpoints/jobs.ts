import { z } from 'zod';
import type { EndpointDefinition } from '../schemas';
import {
    TriggerJobRequest,
    CreateWebhookIpRuleRequest,
    CroniqTriggerSeedDefinition,
    ExecutionStatus,
    IssueApiKeyRequest,
    IssueTokenRequest,
    PasswordChangePasswordRequest,
    PasswordLoginRequest,
    PasswordLogoutRequest,
    PasswordRefreshRequest,
    RotateWebhookSecretRequest,
    UpsertApiClientRequest,
    UpsertJobRequest,
    UpsertTenantRequest,
    UpsertWebhookEndpointRequest,
} from '../schemas';

export const JobsApi: EndpointDefinition[] = [
    {
        method: 'post',
        path: '/jobs/trigger',
        description: `Executes a job immediately or schedules a one-off run when DelaySeconds is provided.`,
        requestFormat: 'json',
        parameters: [{ name: 'body', type: 'Body', schema: TriggerJobRequest }],
        response: z.void(),
    },
] as const;

export type JobsApiEndpoint = (typeof JobsApi)[number];
