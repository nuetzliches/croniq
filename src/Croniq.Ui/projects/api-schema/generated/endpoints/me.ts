import { z } from 'zod';
import type { EndpointDefinition } from '../schemas';

import {
    CreateWebhookIpRuleRequest,
    ExecutionStatus,
    IssueApiKeyRequest,
    IssueTokenRequest,
    RotateWebhookSecretRequest,
    TriggerJobRequest,
    UpsertApiClientRequest,
    UpsertJobRequest,
    UpsertScheduleRequest,
    UpsertTenantRequest,
    UpsertWebhookEndpointRequest,
} from '../schemas';

export const MeApi: EndpointDefinition[] = [
    {
        method: 'get',
        path: '/me',
        requestFormat: 'json',
        response: z.void(),
    },
] as const;

export type MeApiEndpoint = (typeof MeApi)[number];
