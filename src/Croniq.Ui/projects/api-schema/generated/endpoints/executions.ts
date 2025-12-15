import { z } from 'zod';

import type { EndpointDefinition } from '../schemas';

export const ExecutionsApi: EndpointDefinition[] = [
    {
        method: 'get',
        path: '/tenants/:tenantId/executions/:executionId/logs',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
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
