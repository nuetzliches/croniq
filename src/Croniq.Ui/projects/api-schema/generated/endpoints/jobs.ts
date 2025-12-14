import { z } from 'zod';

import type { EndpointDefinition } from '../schemas';
import {
    TriggerJobRequest,
    CreateWebhookIpRuleRequest,
    IssueApiKeyRequest,
    RotateWebhookSecretRequest,
    UpsertScheduleRequest,
    UpsertWebhookEndpointRequest,
} from '../schemas';

export const JobsApi: EndpointDefinition[] = [
    {
        method: 'post',
        path: '/jobs/trigger',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: TriggerJobRequest,
            },
        ],
        response: z.void(),
    },
] as const;

export type JobsApiEndpoint = (typeof JobsApi)[number];
