import type { OpenAPIRegistry } from '@asteasolutions/zod-to-openapi';
import { z } from 'zod';

const paramsSchema = z.object({ executionId: z.string().min(1) });

export function registerDomain(registry: OpenAPIRegistry): void {
    registry.registerPath({
        method: 'get',
        path: '/executions/{executionId}/logs',
        summary: 'Fetch execution logs',
        tags: ['Executions'],
        request: {
            params: paramsSchema,
        },
        responses: {
            200: { description: 'Execution logs returned' },
        },
    });
}
