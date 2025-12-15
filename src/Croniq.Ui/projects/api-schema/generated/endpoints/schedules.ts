import { z } from 'zod';

import type { EndpointDefinition } from '../schemas';
import {
    UpsertScheduleRequest
} from '../schemas';

export const SchedulesApi: EndpointDefinition[] = [
    {
        method: 'post',
        path: '/tenants/:tenantId/schedules',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
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
