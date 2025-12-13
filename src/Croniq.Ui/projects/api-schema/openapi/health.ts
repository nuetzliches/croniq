import type { OpenAPIRegistry } from '@asteasolutions/zod-to-openapi';

export function registerDomain(registry: OpenAPIRegistry): void {
    registry.registerPath({
        method: 'get',
        path: '/health',
        summary: 'Health probe',
        tags: ['Health'],
        responses: {
            200: {
                description: 'Service is healthy',
            },
        },
    });

    registry.registerPath({
        method: 'get',
        path: '/health/persistence',
        summary: 'Persistence health probe',
        tags: ['Health'],
        responses: {
            200: {
                description: 'Persistence layer is healthy',
            },
        },
    });
}
