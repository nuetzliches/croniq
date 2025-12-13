import type { OpenAPIRegistry } from '@asteasolutions/zod-to-openapi';
import { z } from 'zod';

import { issueApiKeyRequestSchema } from '../src/api-keys';

const tenantParams = z.object({ tenantId: z.string().min(1) });
const tenantKeyParams = tenantParams.extend({ keyId: z.string().min(1) });
const optionalEnvironmentQuery = z.object({ environment: z.string().min(1).optional() });

export function registerDomain(registry: OpenAPIRegistry): void {
    registry.register('IssueApiKeyRequest', issueApiKeyRequestSchema);

    registry.registerPath({
        method: 'post',
        path: '/tenants/{tenantId}/api-keys',
        summary: 'Issue a new API key',
        tags: ['Tenant API Keys'],
        request: {
            params: tenantParams,
            body: {
                description: 'API key issuance payload',
                required: true,
                content: {
                    'application/json': { schema: issueApiKeyRequestSchema },
                },
            },
        },
        responses: {
            200: { description: 'API key issued' },
        },
    });

    registry.registerPath({
        method: 'post',
        path: '/tenants/{tenantId}/api-keys/{keyId}/rotate',
        summary: 'Rotate an API key',
        tags: ['Tenant API Keys'],
        request: {
            params: tenantKeyParams,
            query: optionalEnvironmentQuery,
        },
        responses: {
            200: { description: 'API key rotated' },
        },
    });

    registry.registerPath({
        method: 'delete',
        path: '/tenants/{tenantId}/api-keys/{keyId}',
        summary: 'Delete an API key',
        tags: ['Tenant API Keys'],
        request: {
            params: tenantKeyParams,
            query: optionalEnvironmentQuery,
        },
        responses: {
            200: { description: 'API key deleted' },
        },
    });
}
