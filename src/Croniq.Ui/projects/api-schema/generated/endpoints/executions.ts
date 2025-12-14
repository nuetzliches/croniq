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

export const ExecutionsApi: EndpointDefinition[] = [
    {
        method: 'get',
        path: '/executions/:executionId/logs',
        requestFormat: 'json',
        parameters: [
            {
                name: 'executionId',
                type: 'Path',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
] as const;

export type ExecutionsApiEndpoint = (typeof ExecutionsApi)[number];
