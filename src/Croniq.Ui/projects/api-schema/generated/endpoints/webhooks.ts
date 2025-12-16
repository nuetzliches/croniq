import { z } from 'zod';
import type { EndpointDefinition } from '../schemas';
import {
    CreateWebhookIpRuleRequest,
    ExecutionStatus,
    IssueApiKeyRequest,
    IssueTokenRequest,
    PasswordLoginRequest,
    PasswordLogoutRequest,
    PasswordRefreshRequest,
    RotateWebhookSecretRequest,
    TriggerJobRequest,
    UpsertApiClientRequest,
    UpsertJobRequest,
    UpsertScheduleRequest,
    UpsertTenantRequest,
    UpsertWebhookEndpointRequest,
} from '../schemas';

export const WebhooksApi: EndpointDefinition[] = [
    {
        method: 'post',
        path: '/webhooks/:hookKey',
        requestFormat: 'json',
        parameters: [{ name: 'hookKey', type: 'Path', schema: z.string() }],
        response: z.void(),
    },
] as const;

export type WebhooksApiEndpoint = (typeof WebhooksApi)[number];
