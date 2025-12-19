import { z } from 'zod';
import type { EndpointDefinition } from '../schemas';
import {
    TriggerJobRequest,
    CreateWebhookIpRuleRequest,
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
    UpsertScheduleRequest,
    UpsertTenantRequest,
    UpsertWebhookEndpointRequest,
} from '../schemas';

export const JobsApi: EndpointDefinition[] = [
    {
        method: 'post',
        path: '/jobs/trigger',
        description: `Executes a job immediately using the provided metadata and job key.`,
        requestFormat: 'json',
        parameters: [{ name: 'body', type: 'Body', schema: TriggerJobRequest }],
        response: z.void(),
    },
] as const;

export type JobsApiEndpoint = (typeof JobsApi)[number];
