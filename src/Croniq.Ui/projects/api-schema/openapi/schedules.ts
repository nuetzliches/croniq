import type { OpenAPIRegistry } from '@asteasolutions/zod-to-openapi';

import { scheduleListResponseSchema, scheduleSummarySchema, upsertScheduleRequestSchema } from '../src/schedules';

export function registerDomain(registry: OpenAPIRegistry): void {
    registry.register('ScheduleSummary', scheduleSummarySchema);
    registry.register('ScheduleListResponse', scheduleListResponseSchema);
    registry.register('UpsertScheduleRequest', upsertScheduleRequestSchema);

    registry.registerPath({
        method: 'get',
        path: '/schedules',
        summary: 'List schedules',
        description: 'Returns schedules scoped to the active tenant',
        tags: ['Schedules'],
        responses: {
            200: {
                description: 'Schedules collection',
                content: {
                    'application/json': {
                        schema: scheduleListResponseSchema,
                    },
                },
            },
        },
    });

    registry.registerPath({
        method: 'post',
        path: '/schedules',
        summary: 'Create or update a schedule',
        tags: ['Schedules'],
        request: {
            body: {
                description: 'Schedule definition payload',
                required: true,
                content: {
                    'application/json': {
                        schema: upsertScheduleRequestSchema,
                    },
                },
            },
        },
        responses: {
            200: {
                description: 'Schedule accepted',
            },
        },
    });
}
