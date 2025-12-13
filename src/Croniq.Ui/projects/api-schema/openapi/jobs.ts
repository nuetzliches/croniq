import type { OpenAPIRegistry } from '@asteasolutions/zod-to-openapi';

import { triggerJobRequestSchema } from '../src/jobs';

export function registerDomain(registry: OpenAPIRegistry): void {
    registry.register('TriggerJobRequest', triggerJobRequestSchema);

    registry.registerPath({
        method: 'post',
        path: '/jobs/trigger',
        summary: 'Trigger a job execution',
        tags: ['Jobs'],
        request: {
            body: {
                description: 'Job trigger payload',
                required: true,
                content: {
                    'application/json': {
                        schema: triggerJobRequestSchema,
                    },
                },
            },
        },
        responses: {
            200: {
                description: 'Trigger accepted',
            },
        },
    });
}
