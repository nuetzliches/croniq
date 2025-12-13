import type { OpenAPIRegistry } from '@asteasolutions/zod-to-openapi';
import { z } from 'zod';

const paramsSchema = z.object({ hookKey: z.string().min(1) });

export function registerDomain(registry: OpenAPIRegistry): void {
    registry.registerPath({
        method: 'post',
        path: '/webhooks/{hookKey}',
        summary: 'Invoke a webhook endpoint manually',
        tags: ['Webhook Events'],
        request: {
            params: paramsSchema,
        },
        responses: {
            200: { description: 'Invocation accepted' },
        },
    });
}
