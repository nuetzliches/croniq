import { z } from 'zod';
import type { EndpointDefinition } from '../schemas';

import {
    UpsertTenantRequest,
    UpsertJobRequest,
    UpsertScheduleRequest,
    UpsertWebhookEndpointRequest,
    RotateWebhookSecretRequest,
    CreateWebhookIpRuleRequest,
    UpsertApiClientRequest,
    IssueApiKeyRequest,
    IssueTokenRequest,
    ExecutionStatus,
    TriggerJobRequest,
} from '../schemas';

export const TenantsApi: EndpointDefinition[] = [
    {
        method: 'get',
        path: '/tenants',
        requestFormat: 'json',
        parameters: [
            {
                name: 'state',
                type: 'Query',
                schema: z.string().optional(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/tenants',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: UpsertTenantRequest,
            },
        ],
        response: z.void(),
    },
    {
        method: 'get',
        path: '/tenants/:tenantId',
        requestFormat: 'json',
        parameters: [
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
        path: '/tenants/:tenantId',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'get',
        path: '/tenants/:tenantId/api-clients',
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
                schema: z.string().optional(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/tenants/:tenantId/api-clients',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: UpsertApiClientRequest,
            },
            {
                name: 'tenantId',
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
        method: 'delete',
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
        path: '/tenants/:tenantId/api-clients/:clientId/tokens',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: IssueTokenRequest,
            },
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
        path: '/tenants/:tenantId/executions',
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
            {
                name: 'jobKey',
                type: 'Query',
                schema: z.string().optional(),
            },
            {
                name: 'status',
                type: 'Query',
                schema: z
                    .union([z.literal(0), z.literal(1), z.literal(2)])
                    .optional(),
            },
            {
                name: 'startedAfterUtc',
                type: 'Query',
                schema: z.string().datetime({ offset: true }).optional(),
            },
            {
                name: 'startedBeforeUtc',
                type: 'Query',
                schema: z.string().datetime({ offset: true }).optional(),
            },
            {
                name: 'limit',
                type: 'Query',
                schema: z.number().int().optional(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'get',
        path: '/tenants/:tenantId/executions/:executionId',
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
    {
        method: 'get',
        path: '/tenants/:tenantId/jobs',
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
        path: '/tenants/:tenantId/jobs',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: UpsertJobRequest,
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
        ],
        response: z.void(),
    },
    {
        method: 'get',
        path: '/tenants/:tenantId/jobs/:jobId',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'jobId',
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
        path: '/tenants/:tenantId/jobs/:jobId',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'jobId',
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
        path: '/tenants/:tenantId/schedules',
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
            {
                name: 'jobKey',
                type: 'Query',
                schema: z.string().optional(),
            },
        ],
        response: z.void(),
    },
    {
        method: 'post',
        path: '/tenants/:tenantId/schedules',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: UpsertScheduleRequest,
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
        method: 'get',
        path: '/tenants/:tenantId/schedules/:triggerId',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'triggerId',
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
        path: '/tenants/:tenantId/schedules/:triggerId',
        requestFormat: 'json',
        parameters: [
            {
                name: 'tenantId',
                type: 'Path',
                schema: z.string(),
            },
            {
                name: 'triggerId',
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
        path: '/tenants/:tenantId/tokens',
        requestFormat: 'json',
        parameters: [
            {
                name: 'body',
                type: 'Body',
                schema: IssueTokenRequest,
            },
            {
                name: 'tenantId',
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
