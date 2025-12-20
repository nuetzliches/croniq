import { z } from 'zod';
import type { EndpointDefinition } from '../schemas';
import {
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
    TriggerJobRequest,
    UpsertApiClientRequest,
    UpsertJobRequest,
    UpsertTenantRequest,
    UpsertWebhookEndpointRequest,
} from '../schemas';

export const MeApi: EndpointDefinition[] = [
    {
        method: 'get',
        path: '/me',
        description: `Returns the current caller context (tenant, environment, scopes) after authentication.`,
        requestFormat: 'json',
        response: z.void(),
    },
] as const;

export type MeApiEndpoint = (typeof MeApi)[number];
