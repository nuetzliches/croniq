import { z } from 'zod';

import type { EndpointDefinition } from '../schemas';
import {
    UpsertWebhookEndpointRequest,
    RotateWebhookSecretRequest,
    CreateWebhookIpRuleRequest,
    IssueApiKeyRequest,
    TriggerJobRequest,
    UpsertScheduleRequest,
} from '../schemas';

export const TenantsApi: EndpointDefinition[] = [
    {
        method: 'get',
        path: '/tenants/:tenantId/api-clients/:clientId',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'clientId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string().optional(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/tenants/:tenantId/api-keys',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: IssueApiKeyRequest,
            },
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'delete',
        path: '/tenants/:tenantId/api-keys/:keyId',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'keyId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string().optional(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/tenants/:tenantId/api-keys/:keyId/rotate',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'keyId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string().optional(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'get',
        path: '/tenants/:tenantId/webhooks',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/tenants/:tenantId/webhooks',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: UpsertWebhookEndpointRequest,
            },
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string(),
            },
            {
                name: 'allowUnsigned',
                type: 'Query',
                schema: z.boolean(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'delete',
        path: '/tenants/:tenantId/webhooks/:hookKey',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'hookKey',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'get',
        path: '/tenants/:tenantId/webhooks/:hookKey/ip-rules',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'hookKey',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/tenants/:tenantId/webhooks/:hookKey/ip-rules',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: CreateWebhookIpRuleRequest,
            },
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'hookKey',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'delete',
        path: '/tenants/:tenantId/webhooks/:hookKey/ip-rules/:ruleId',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'hookKey',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'ruleId',
                type: 'Path',
                schema: z.number().int(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/tenants/:tenantId/webhooks/:hookKey/rotate-secret',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: RotateWebhookSecretRequest,
            },
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'hookKey',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'get',
        path: '/tenants/:tenantId/webhooks/deadletters',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/tenants/:tenantId/webhooks/deadletters/:deadLetterId/replay',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'deadLetterId',
                type: 'Path',
                schema: z.number().int(),
            },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
] as const;

export type TenantsApiEndpoint = (typeof TenantsApi)[number];
