import { z } from 'zod';
import type { EndpointDefinition } from '../schemas';
import {
    CreateWebhookIpRuleRequest,
    CroniqTriggerSeedDefinition,
    ExecutionStatus,
    IssueApiKeyRequest,
    IssueTokenRequest,
    PasswordChangePasswordRequest,
    PasswordLoginRequest,
    PasswordLogoutRequest,
    PasswordRefreshRequest,
    RotateWebhookSecretRequest,
    RunnerHeartbeatRequest,
    ScheduleDeadLetterResponse,
    ScheduleReplayResult,
    ScheduleResponse,
    ScheduleUpsertResult,
    TriggerJobRequest,
    UpsertApiClientRequest,
    UpsertJobRequest,
    UpsertTenantRequest,
    UpsertWebhookEndpointRequest,
    WorkLeaseToken,
    WorkAckRequest,
    WorkEventEntry,
    WorkEventsRequest,
    WorkPollRequest,
    WorkRenewRequest,
} from '../schemas';

export const HealthApi: EndpointDefinition[] = [
    {
        method: 'get',
        path: '/health',
        description: `Returns 200 when the Croniq API process is alive.`,
        requestFormat: 'json',
        response: z.void(),
    },
    {
        method: 'get',
        path: '/health/persistence',
        description: `Checks the configured job persistence provider for reachability.`,
        requestFormat: 'json',
        response: z.void(),
    },
] as const;

export type HealthApiEndpoint = (typeof HealthApi)[number];
