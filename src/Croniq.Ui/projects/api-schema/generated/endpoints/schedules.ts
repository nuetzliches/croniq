import { z } from 'zod';

import type { EndpointDefinition } from '../schemas';
import {
    UpsertScheduleRequest,
    CreateWebhookIpRuleRequest,
    IssueApiKeyRequest,
    RotateWebhookSecretRequest,
    TriggerJobRequest,
    UpsertWebhookEndpointRequest,
} from '../schemas';

export const SchedulesApi: EndpointDefinition[] = [
    {
        method: 'post',
        path: '/schedules',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: UpsertScheduleRequest,
            },
        ],
        response: z.void(),
    },
] as const;

export type SchedulesApiEndpoint = (typeof SchedulesApi)[number];
