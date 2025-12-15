import { z } from 'zod'; import type { EndpointDefinition } from '../schemas';
  import {
    TriggerJobRequest,
    UpsertScheduleRequest,
    UpsertWebhookEndpointRequest,
    RotateWebhookSecretRequest,
    CreateWebhookIpRuleRequest,
    IssueApiKeyRequest,
    ScheduleSummary,
    ScheduleListResponse,
  } from '../schemas';

export const
HealthApi: EndpointDefinition[] = [
  { method: 'get', path: '/health',
    requestFormat: 'json',
  response:
  z.void(),
  },
  { method: 'get', path: '/health/persistence',
    requestFormat: 'json',
  response:
  z.void(),
  },
] as const; export type
HealthApiEndpoint = (typeof
HealthApi)[number];