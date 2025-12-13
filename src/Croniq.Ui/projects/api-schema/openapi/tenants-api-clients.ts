import type { OpenAPIRegistry } from '@asteasolutions/zod-to-openapi';
import { z } from 'zod';

const paramsSchema = z.object({
    tenantId: z.string().min(1),
    clientId: z.string().min(1),
});
const optionalEnvironmentQuery = z.object({ environment: z.string().min(1).optional() });

export function registerDomain(registry: OpenAPIRegistry): void {
    registry.registerPath({
        method: 'get',
        path: '/tenants/{tenantId}/api-clients/{clientId}',
        summary: 'Lookup an API client',
        tags: ['Tenant API Clients'],
        request: {
            params: paramsSchema,
            query: optionalEnvironmentQuery,
        },
        responses: {
            200: { description: 'API client returned' },
        },
    });
}
