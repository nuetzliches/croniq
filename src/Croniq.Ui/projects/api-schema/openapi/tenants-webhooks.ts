import type { OpenAPIRegistry } from '@asteasolutions/zod-to-openapi';
import { z } from 'zod';

import {
    createWebhookIpRuleRequestSchema,
    rotateWebhookSecretRequestSchema,
    upsertWebhookEndpointRequestSchema,
} from '../src/webhooks';

const tenantParams = z.object({ tenantId: z.string().min(1) });
const hookParams = tenantParams.extend({ hookKey: z.string().min(1) });
const ruleParams = hookParams.extend({ ruleId: z.string().min(1) });
const deadLetterParams = tenantParams.extend({ deadLetterId: z.string().min(1) });

const environmentQuery = z.object({ environment: z.string().min(1) });
const environmentWithUnsignedQuery = environmentQuery.extend({ allowUnsigned: z.boolean() });

export function registerDomain(registry: OpenAPIRegistry): void {
    registry.register('UpsertWebhookEndpointRequest', upsertWebhookEndpointRequestSchema);
    registry.register('RotateWebhookSecretRequest', rotateWebhookSecretRequestSchema);
    registry.register('CreateWebhookIpRuleRequest', createWebhookIpRuleRequestSchema);

    registry.registerPath({
        method: 'get',
        path: '/tenants/{tenantId}/webhooks',
        summary: 'List webhook endpoints for a tenant',
        tags: ['Tenant Webhooks'],
        request: {
            params: tenantParams,
            query: environmentQuery,
        },
        responses: {
            200: { description: 'Webhook endpoints returned' },
        },
    });

    registry.registerPath({
        method: 'post',
        path: '/tenants/{tenantId}/webhooks',
        summary: 'Create or update a webhook endpoint',
        tags: ['Tenant Webhooks'],
        request: {
            params: tenantParams,
            query: environmentWithUnsignedQuery,
            body: {
                description: 'Webhook endpoint payload',
                required: true,
                content: {
                    'application/json': { schema: upsertWebhookEndpointRequestSchema },
                },
            },
        },
        responses: {
            200: { description: 'Webhook endpoint accepted' },
        },
    });

    registry.registerPath({
        method: 'delete',
        path: '/tenants/{tenantId}/webhooks/{hookKey}',
        summary: 'Delete a webhook endpoint',
        tags: ['Tenant Webhooks'],
        request: {
            params: hookParams,
            query: environmentQuery,
        },
        responses: {
            200: { description: 'Webhook endpoint deleted' },
        },
    });

    registry.registerPath({
        method: 'post',
        path: '/tenants/{tenantId}/webhooks/{hookKey}/rotate-secret',
        summary: 'Rotate webhook signing secret',
        tags: ['Tenant Webhooks'],
        request: {
            params: hookParams,
            query: environmentQuery,
            body: {
                description: 'Rotation configuration',
                required: true,
                content: {
                    'application/json': { schema: rotateWebhookSecretRequestSchema },
                },
            },
        },
        responses: {
            200: { description: 'Secret rotation scheduled' },
        },
    });

    registry.registerPath({
        method: 'get',
        path: '/tenants/{tenantId}/webhooks/{hookKey}/ip-rules',
        summary: 'List webhook IP rules',
        tags: ['Tenant Webhooks'],
        request: {
            params: hookParams,
            query: environmentQuery,
        },
        responses: {
            200: { description: 'IP rules returned' },
        },
    });

    registry.registerPath({
        method: 'post',
        path: '/tenants/{tenantId}/webhooks/{hookKey}/ip-rules',
        summary: 'Create a webhook IP rule',
        tags: ['Tenant Webhooks'],
        request: {
            params: hookParams,
            query: environmentQuery,
            body: {
                description: 'IP rule definition',
                required: true,
                content: {
                    'application/json': { schema: createWebhookIpRuleRequestSchema },
                },
            },
        },
        responses: {
            200: { description: 'IP rule accepted' },
        },
    });

    registry.registerPath({
        method: 'delete',
        path: '/tenants/{tenantId}/webhooks/{hookKey}/ip-rules/{ruleId}',
        summary: 'Delete a webhook IP rule',
        tags: ['Tenant Webhooks'],
        request: {
            params: ruleParams,
            query: environmentQuery,
        },
        responses: {
            200: { description: 'IP rule deleted' },
        },
    });

    registry.registerPath({
        method: 'get',
        path: '/tenants/{tenantId}/webhooks/deadletters',
        summary: 'List webhook dead letters',
        tags: ['Tenant Webhooks'],
        request: {
            params: tenantParams,
            query: environmentQuery,
        },
        responses: {
            200: { description: 'Dead letters returned' },
        },
    });

    registry.registerPath({
        method: 'post',
        path: '/tenants/{tenantId}/webhooks/deadletters/{deadLetterId}/replay',
        summary: 'Replay a webhook dead letter',
        tags: ['Tenant Webhooks'],
        request: {
            params: deadLetterParams,
            query: environmentQuery,
        },
        responses: {
            200: { description: 'Replay scheduled' },
        },
    });
}
