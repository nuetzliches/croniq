import type { OpenAPIRegistry } from '@asteasolutions/zod-to-openapi';

export function registerDomain(registry: OpenAPIRegistry): void {
    registry.registerComponent('securitySchemes', 'X-Croniq-Key', {
        type: 'apiKey',
        description: 'Croniq API key passed via X-Croniq-Key header.',
        name: 'X-Croniq-Key',
        in: 'header',
    });
}
