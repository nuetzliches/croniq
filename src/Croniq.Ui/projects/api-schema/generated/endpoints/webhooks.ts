import { z } from 'zod';

import type { EndpointDefinition } from '../schemas';
import {
    CreateWebhookIpRuleRequest,
    IssueApiKeyRequest,
    RotateWebhookSecretRequest,
    TriggerJobRequest,
    UpsertScheduleRequest,
    UpsertWebhookEndpointRequest,
} from '../schemas';

export const WebhooksApi: EndpointDefinition[] = [
    {
        method: 'post',
        path: '/webhooks/:hookKey',
        requestFormat: 'json',
        parameters: [
            {
                name: 'hookKey',
                type: 'Path',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
] as const;

export type WebhooksApiEndpoint = (typeof WebhooksApi)[number];
