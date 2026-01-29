import { z } from 'zod';
export const HealthStatusResponse = z
    .object({ status: z.string().nullable() })
    .partial();
export type HealthStatusResponse = z.infer<typeof HealthStatusResponse>;
export const PersistenceHealthResponse = z
    .object({
        status: z.string().nullable(),
        provider: z.string().nullable(),
        note: z.string().nullable(),
        db: z.string().nullable(),
    })
    .partial();
export type PersistenceHealthResponse = z.infer<
    typeof PersistenceHealthResponse
>;
export const CallerType = z.union([z.literal(0), z.literal(1)]);
export type CallerType = z.infer<typeof CallerType>;
export const CallerInfoResponse = z
    .object({
        tenantId: z.string().nullable(),
        environmentTag: z.string().nullable(),
        callerId: z.string().nullable(),
        callerType: CallerType,
        scopes: z.array(z.string()).nullable(),
        isActive: z.boolean(),
    })
    .partial();
export type CallerInfoResponse = z.infer<typeof CallerInfoResponse>;
export const UpsertTenantRequest = z.object({
    tenantId: z.string().min(1),
    name: z.string().min(1),
});
export type UpsertTenantRequest = z.infer<typeof UpsertTenantRequest>;
export const TenantResponse = z
    .object({
        tenantId: z.string().nullable(),
        name: z.string().nullable(),
        isActive: z.boolean(),
        createdAtUtc: z.iso.datetime({ offset: true }),
    })
    .partial();
export type TenantResponse = z.infer<typeof TenantResponse>;
export const JobResponse = z
    .object({
        jobKey: z.string().nullable(),
        namespace: z.string().nullable(),
        name: z.string().nullable(),
        variant: z.string().nullable(),
        description: z.string().nullable(),
        metadata: z.record(z.string(), z.string()).nullable(),
        assignedRunnerId: z.string().nullable(),
        assignedBy: z.string().nullable(),
        assignedAtUtc: z.iso.datetime({ offset: true }).nullable(),
        assignmentSource: z.string().nullable(),
        assignmentNotes: z.string().nullable(),
        isActive: z.boolean(),
    })
    .partial();
export type JobResponse = z.infer<typeof JobResponse>;
export const UpsertJobRequest = z.object({
    jobKey: z.string().min(1),
    namespace: z.string().min(1),
    name: z.string().min(1),
    variant: z.string().nullish(),
    description: z.string().nullish(),
    metadata: z.record(z.string(), z.string()).nullish(),
    isActive: z.boolean().nullish(),
    assignedRunnerId: z.string().nullish(),
    assignmentNotes: z.string().nullish(),
});
export type UpsertJobRequest = z.infer<typeof UpsertJobRequest>;
export const ExecutionKind = z.union([z.literal(0), z.literal(1)]);
export type ExecutionKind = z.infer<typeof ExecutionKind>;
export const ExecutionStatus = z.union([
    z.literal(0),
    z.literal(1),
    z.literal(2),
]);
export type ExecutionStatus = z.infer<typeof ExecutionStatus>;
export const ExecutionResponse = z
    .object({
        executionId: z.string().nullable(),
        jobKey: z.string().nullable(),
        tenantId: z.string().nullable(),
        environmentTag: z.string().nullable(),
        kind: ExecutionKind,
        status: ExecutionStatus,
        fireAtUtc: z.iso.datetime({ offset: true }),
        startedAtUtc: z.iso.datetime({ offset: true }),
        completedAtUtc: z.iso.datetime({ offset: true }).nullable(),
        durationMs: z.number().nullable(),
        triggerId: z.string().nullable(),
        instanceId: z.string().nullable(),
        traceId: z.string().nullable(),
        correlationId: z.string().nullable(),
        errorType: z.string().nullable(),
        errorMessage: z.string().nullable(),
        executionMode: z.string().nullable(),
        invocationSource: z.string().nullable(),
    })
    .partial();
export type ExecutionResponse = z.infer<typeof ExecutionResponse>;
export const CroniqTriggerSeedDefinition = z
    .object({
        triggerId: z.string().nullable(),
        jobKey: z.string().nullable(),
        cronExpression: z.string().nullable(),
        startAtUtc: z.iso.datetime({ offset: true }).nullable(),
        endAtUtc: z.iso.datetime({ offset: true }).nullable(),
        enabled: z.boolean(),
        metadata: z.record(z.string(), z.string()).nullable(),
        description: z.string().nullable(),
        managedBy: z.string().nullable(),
        timeZoneId: z.string().nullable(),
        calendarId: z.string().nullable(),
    })
    .partial();
export type CroniqTriggerSeedDefinition = z.infer<
    typeof CroniqTriggerSeedDefinition
>;
export const ScheduleUpsertResult = z
    .object({
        triggerId: z.string().nullable(),
        jobKey: z.string().nullable(),
        scheduleExpression: z.string().nullable(),
        calendarId: z.string().nullable(),
    })
    .partial();
export type ScheduleUpsertResult = z.infer<typeof ScheduleUpsertResult>;
export const ScheduleResponse = z
    .object({
        triggerId: z.string().nullable(),
        jobKey: z.string().nullable(),
        cronExpression: z.string().nullable(),
        tenantId: z.string().nullable(),
        environmentTag: z.string().nullable(),
        startAtUtc: z.iso.datetime({ offset: true }).nullable(),
        endAtUtc: z.iso.datetime({ offset: true }).nullable(),
        enabled: z.boolean(),
        metadata: z.record(z.string(), z.string()).nullable(),
        timeZoneId: z.string().nullable(),
        calendarId: z.string().nullable(),
    })
    .partial();
export type ScheduleResponse = z.infer<typeof ScheduleResponse>;
export const CalendarMode = z.union([z.literal(0), z.literal(1)]);
export type CalendarMode = z.infer<typeof CalendarMode>;
export const CalendarRuleType = z.union([
    z.literal(0),
    z.literal(1),
    z.literal(2),
    z.literal(3),
    z.literal(4),
]);
export type CalendarRuleType = z.infer<typeof CalendarRuleType>;
export const CalendarDailyWindowRule = z
    .object({
        startTime: z.string().nullable(),
        endTime: z.string().nullable(),
        daysOfWeek: z.array(z.string()).nullable(),
    })
    .partial();
export type CalendarDailyWindowRule = z.infer<typeof CalendarDailyWindowRule>;
export const CalendarWeeklyWindowRule = z
    .object({ daysOfWeek: z.array(z.string()).nullable() })
    .partial();
export type CalendarWeeklyWindowRule = z.infer<typeof CalendarWeeklyWindowRule>;
export const CalendarAnnualDateListRule = z
    .object({ monthDays: z.array(z.string()).nullable() })
    .partial();
export type CalendarAnnualDateListRule = z.infer<
    typeof CalendarAnnualDateListRule
>;
export const CalendarDateListRule = z
    .object({ dates: z.array(z.string()).nullable() })
    .partial();
export type CalendarDateListRule = z.infer<typeof CalendarDateListRule>;
export const CalendarCronRule = z
    .object({ cronExpression: z.string().nullable() })
    .partial();
export type CalendarCronRule = z.infer<typeof CalendarCronRule>;
export const CalendarRuleDefinition = z
    .object({
        ruleId: z.string().nullable(),
        ruleType: CalendarRuleType,
        sortOrder: z.number().int(),
        isEnabled: z.boolean(),
        dailyWindow: CalendarDailyWindowRule,
        weeklyWindow: CalendarWeeklyWindowRule,
        annualDateList: CalendarAnnualDateListRule,
        dateList: CalendarDateListRule,
        cronRule: CalendarCronRule,
    })
    .partial();
export type CalendarRuleDefinition = z.infer<typeof CalendarRuleDefinition>;
export const CalendarResponse = z
    .object({
        calendarId: z.string().nullable(),
        tenantId: z.string().nullable(),
        environmentTag: z.string().nullable(),
        name: z.string().nullable(),
        description: z.string().nullable(),
        timeZoneId: z.string().nullable(),
        mode: CalendarMode,
        rules: z.array(CalendarRuleDefinition).nullable(),
        enabled: z.boolean(),
        createdAtUtc: z.iso.datetime({ offset: true }),
        updatedAtUtc: z.iso.datetime({ offset: true }),
    })
    .partial();
export type CalendarResponse = z.infer<typeof CalendarResponse>;
export const CroniqCalendarSeedDefinition = z
    .object({
        calendarId: z.string().nullable(),
        name: z.string().nullable(),
        description: z.string().nullable(),
        timeZoneId: z.string().nullable(),
        mode: CalendarMode,
        enabled: z.boolean(),
        rules: z.array(CalendarRuleDefinition).nullable(),
    })
    .partial();
export type CroniqCalendarSeedDefinition = z.infer<
    typeof CroniqCalendarSeedDefinition
>;
export const CalendarUpsertResult = z
    .object({ calendarId: z.string().nullable(), name: z.string().nullable() })
    .partial();
export type CalendarUpsertResult = z.infer<typeof CalendarUpsertResult>;
export const ScheduleDeadLetterResponse = z
    .object({
        id: z.number().int(),
        triggerId: z.string().nullable(),
        jobKey: z.string().nullable(),
        tenantId: z.string().nullable(),
        environmentTag: z.string().nullable(),
        fireAtUtc: z.iso.datetime({ offset: true }),
        reason: z.string().nullable(),
        payload: z.string().nullable(),
        metadata: z.record(z.string(), z.string()).nullable(),
        createdAtUtc: z.iso.datetime({ offset: true }),
        expiresAtUtc: z.iso.datetime({ offset: true }).nullable(),
    })
    .partial();
export type ScheduleDeadLetterResponse = z.infer<
    typeof ScheduleDeadLetterResponse
>;
export const ScheduleReplayResult = z
    .object({
        status: z.string().nullable(),
        id: z.number().int(),
        jobKey: z.string().nullable(),
        triggerId: z.string().nullable(),
    })
    .partial();
export type ScheduleReplayResult = z.infer<typeof ScheduleReplayResult>;
export const ScheduleForecastBucket = z
    .object({
        startAtUtc: z.iso.datetime({ offset: true }),
        endAtUtc: z.iso.datetime({ offset: true }),
        count: z.number().int(),
    })
    .partial();
export type ScheduleForecastBucket = z.infer<typeof ScheduleForecastBucket>;
export const ScheduleForecastSummary = z
    .object({ windowMinutes: z.number().int(), count: z.number().int() })
    .partial();
export type ScheduleForecastSummary = z.infer<typeof ScheduleForecastSummary>;
export const ScheduleForecastResponse = z
    .object({
        generatedAtUtc: z.iso.datetime({ offset: true }),
        windowStartUtc: z.iso.datetime({ offset: true }),
        windowEndUtc: z.iso.datetime({ offset: true }),
        bucketMinutes: z.number().int(),
        buckets: z.array(ScheduleForecastBucket).nullable(),
        summaries: z.array(ScheduleForecastSummary).nullable(),
        totalSchedules: z.number().int(),
        activeSchedules: z.number().int(),
    })
    .partial();
export type ScheduleForecastResponse = z.infer<typeof ScheduleForecastResponse>;
export const WebhookIpRuleResponse = z
    .object({
        id: z.number().int(),
        cidr: z.string().nullable(),
        description: z.string().nullable(),
        createdBy: z.string().nullable(),
        createdAtUtc: z.iso.datetime({ offset: true }),
        updatedAtUtc: z.iso.datetime({ offset: true }),
    })
    .partial();
export type WebhookIpRuleResponse = z.infer<typeof WebhookIpRuleResponse>;
export const WebhookEndpointResponse = z
    .object({
        hookKey: z.string().nullable(),
        jobKey: z.string().nullable(),
        enabled: z.boolean(),
        requireSignature: z.boolean(),
        requestsPerMinute: z.number().int(),
        metadata: z.record(z.string(), z.string()).nullable(),
        ipRules: z.array(WebhookIpRuleResponse).nullable(),
        status: z.string().nullable(),
        lastDeliveryAtUtc: z.iso.datetime({ offset: true }).nullable(),
        ipRuleCount: z.number().int().nullable(),
        createdAtUtc: z.iso.datetime({ offset: true }),
        updatedAtUtc: z.iso.datetime({ offset: true }),
        secret: z.string().nullable(),
    })
    .partial();
export type WebhookEndpointResponse = z.infer<typeof WebhookEndpointResponse>;
export const UpsertWebhookEndpointRequest = z.object({
    hookKey: z.string().min(1),
    jobKey: z.string().min(1),
    enabled: z.boolean().optional(),
    requireSignature: z.boolean().optional(),
    requestsPerMinute: z.number().int().nullish(),
    secret: z.string().nullish(),
    metadata: z.record(z.string(), z.string()).nullish(),
    signatureVersion: z.number().int().optional(),
    allowUnsigned: z.boolean().optional(),
});
export type UpsertWebhookEndpointRequest = z.infer<
    typeof UpsertWebhookEndpointRequest
>;
export const WebhookCapabilitiesResponse = z
    .object({
        allowUnsignedHooks: z.boolean(),
        defaultRequestsPerMinute: z.number().int(),
        mode: z.string().nullable(),
        remoteBaseUrl: z.string().nullable(),
        remoteIngressBaseUrl: z.string().nullable(),
    })
    .partial();
export type WebhookCapabilitiesResponse = z.infer<
    typeof WebhookCapabilitiesResponse
>;
export const WebhookRemoteHealthResponse = z
    .object({
        status: z.string().nullable(),
        checkedAtUtc: z.iso.datetime({ offset: true }),
        statusCode: z.number().int().nullable(),
        detail: z.string().nullable(),
    })
    .partial();
export type WebhookRemoteHealthResponse = z.infer<
    typeof WebhookRemoteHealthResponse
>;
export const RotateWebhookSecretRequest = z
    .object({
        activateInSeconds: z.number().int().nullable(),
        gracePeriodSeconds: z.number().int().nullable(),
        notes: z.string().nullable(),
    })
    .partial();
export type RotateWebhookSecretRequest = z.infer<
    typeof RotateWebhookSecretRequest
>;
export const RotateWebhookSecretResponse = z
    .object({
        hookKey: z.string().nullable(),
        activatedAtUtc: z.iso.datetime({ offset: true }),
        expiresAtUtc: z.iso.datetime({ offset: true }).nullable(),
        secret: z.string().nullable(),
        secretHash: z.string().nullable(),
    })
    .partial();
export type RotateWebhookSecretResponse = z.infer<
    typeof RotateWebhookSecretResponse
>;
export const CreateWebhookIpRuleRequest = z.object({
    cidr: z.string().min(1),
    description: z.string().nullish(),
});
export type CreateWebhookIpRuleRequest = z.infer<
    typeof CreateWebhookIpRuleRequest
>;
export const WebhookEndpointEventResponse = z
    .object({
        id: z.number().int(),
        hookKey: z.string().nullable(),
        eventType: z.string().nullable(),
        occurredAtUtc: z.iso.datetime({ offset: true }),
        actor: z.string().nullable(),
        correlationId: z.string().nullable(),
    })
    .partial();
export type WebhookEndpointEventResponse = z.infer<
    typeof WebhookEndpointEventResponse
>;
export const WebhookInvokeResult = z
    .object({
        status: z.string().nullable(),
        hook: z.string().nullable(),
        job: z.string().nullable(),
        executionId: z.string().nullable(),
    })
    .partial();
export type WebhookInvokeResult = z.infer<typeof WebhookInvokeResult>;
export const WebhookDeadLetterResponse = z
    .object({
        id: z.number().int(),
        hookKey: z.string().nullable(),
        jobKey: z.string().nullable(),
        tenantId: z.string().nullable(),
        environmentTag: z.string().nullable(),
        payload: z.string().nullable(),
        headers: z.record(z.string(), z.string()).nullable(),
        metadata: z.record(z.string(), z.string()).nullable(),
        failureReason: z.string().nullable(),
        attempts: z.number().int(),
        statusCode: z.number().int().nullable(),
        errorDetails: z.string().nullable(),
        createdAtUtc: z.iso.datetime({ offset: true }),
        lastAttemptAtUtc: z.iso.datetime({ offset: true }).nullable(),
        nextAttemptAtUtc: z.iso.datetime({ offset: true }).nullable(),
        expiresAtUtc: z.iso.datetime({ offset: true }).nullable(),
    })
    .partial();
export type WebhookDeadLetterResponse = z.infer<
    typeof WebhookDeadLetterResponse
>;
export const WebhookActivityTimelineEntry = z
    .object({
        id: z.string().nullable(),
        kind: z.string().nullable(),
        status: z.string().nullable(),
        hookKey: z.string().nullable(),
        jobKey: z.string().nullable(),
        environment: z.string().nullable(),
        source: z.string().nullable(),
        occurredAtUtc: z.iso.datetime({ offset: true }),
        latencyMs: z.number().int().nullable(),
        attempts: z.number().int().nullable(),
        payloadBytes: z.number().int().nullable(),
        requestId: z.string().nullable(),
        reason: z.string().nullable(),
        deadLetterId: z.number().int().nullable(),
    })
    .partial();
export type WebhookActivityTimelineEntry = z.infer<
    typeof WebhookActivityTimelineEntry
>;
export const WebhookActivityBucket = z
    .object({
        bucketStartUtc: z.iso.datetime({ offset: true }),
        bucketEndUtc: z.iso.datetime({ offset: true }).nullable(),
        totalCount: z.number().int(),
        errorCount: z.number().int(),
        warningCount: z.number().int(),
        pendingCount: z.number().int(),
        leasedCount: z.number().int(),
        deadLetterCount: z.number().int(),
        p95LatencyMs: z.number().int().nullable(),
    })
    .partial();
export type WebhookActivityBucket = z.infer<typeof WebhookActivityBucket>;
export const WebhookActivitySummary = z
    .object({
        bucketMinutes: z.number().int(),
        windowStartUtc: z.iso.datetime({ offset: true }),
        windowEndUtc: z.iso.datetime({ offset: true }),
        buckets: z.array(WebhookActivityBucket).nullable(),
    })
    .partial();
export type WebhookActivitySummary = z.infer<typeof WebhookActivitySummary>;
export const WebhookReplayResult = z
    .object({
        status: z.string().nullable(),
        hook: z.string().nullable(),
        job: z.string().nullable(),
    })
    .partial();
export type WebhookReplayResult = z.infer<typeof WebhookReplayResult>;
export const WebhookDeadLetterFailureRequest = z.object({
    failureReason: z.string().min(1),
    statusCode: z.number().int().nullish(),
    errorDetails: z.string().nullish(),
    nextAttemptAtUtc: z.iso.datetime({ offset: true }).nullish(),
});
export type WebhookDeadLetterFailureRequest = z.infer<
    typeof WebhookDeadLetterFailureRequest
>;
export const ApiClientResponse = z
    .object({
        clientId: z.string().nullable(),
        tenantId: z.string().nullable(),
        name: z.string().nullable(),
        environmentTag: z.string().nullable(),
        scopes: z.array(z.string()).nullable(),
        isActive: z.boolean(),
        expiresAtUtc: z.iso.datetime({ offset: true }).nullable(),
    })
    .partial();
export type ApiClientResponse = z.infer<typeof ApiClientResponse>;
export const UpsertApiClientRequest = z.object({
    clientId: z.string().min(1),
    name: z.string().nullish(),
    environmentTag: z.string().nullish(),
    scopes: z.array(z.string()).nullish(),
    isActive: z.boolean().nullish(),
});
export type UpsertApiClientRequest = z.infer<typeof UpsertApiClientRequest>;
export const IssueApiKeyRequest = z.object({
    clientId: z.string().min(1),
    environmentTag: z.string().nullish(),
    scopes: z.array(z.string()).nullish(),
    ttlHours: z.number().int().nullish(),
});
export type IssueApiKeyRequest = z.infer<typeof IssueApiKeyRequest>;
export const IssueApiKeyResponse = z
    .object({
        clientId: z.string().nullable(),
        tenantId: z.string().nullable(),
        keyId: z.string().nullable(),
        plaintextSecret: z.string().nullable(),
        expiresAtUtc: z.iso.datetime({ offset: true }).nullable(),
        environmentTag: z.string().nullable(),
    })
    .partial();
export type IssueApiKeyResponse = z.infer<typeof IssueApiKeyResponse>;
export const IssueTokenRequest = z
    .object({
        clientId: z.string().nullable(),
        scopes: z.array(z.string()).nullable(),
        audience: z.string().nullable(),
        ttlMinutes: z.number().int().nullable(),
    })
    .partial();
export type IssueTokenRequest = z.infer<typeof IssueTokenRequest>;
export const IssueTokenResponse = z
    .object({
        accessToken: z.string().nullable(),
        tokenType: z.string().nullable(),
        expiresIn: z.number().int(),
    })
    .partial();
export type IssueTokenResponse = z.infer<typeof IssueTokenResponse>;
export const PasswordLoginRequest = z
    .object({
        username: z.string().nullable(),
        password: z.string().nullable(),
        tenantId: z.string().nullable(),
        environmentTag: z.string().nullable(),
        scopes: z.array(z.string()).nullable(),
        audience: z.string().nullable(),
    })
    .partial();
export type PasswordLoginRequest = z.infer<typeof PasswordLoginRequest>;
export const PasswordAuthResponse = z
    .object({
        tenantId: z.string().nullable(),
        accessToken: z.string().nullable(),
        tokenType: z.string().nullable(),
        expiresIn: z.number().int().nullable(),
        refreshToken: z.string().nullable(),
        passwordChangeRequired: z.boolean(),
    })
    .partial();
export type PasswordAuthResponse = z.infer<typeof PasswordAuthResponse>;
export const PasswordRefreshRequest = z
    .object({
        refreshToken: z.string().nullable(),
        tenantId: z.string().nullable(),
        environmentTag: z.string().nullable(),
        scopes: z.array(z.string()).nullable(),
        audience: z.string().nullable(),
    })
    .partial();
export type PasswordRefreshRequest = z.infer<typeof PasswordRefreshRequest>;
export const OidcAuthResponse = z
    .object({
        accessToken: z.string().nullable(),
        tokenType: z.string().nullable(),
        expiresIn: z.number().int().nullable(),
        tenantId: z.string().nullable(),
    })
    .partial();
export type OidcAuthResponse = z.infer<typeof OidcAuthResponse>;
export const PasswordLogoutRequest = z
    .object({
        refreshToken: z.string().nullable(),
        tenantId: z.string().nullable(),
    })
    .partial();
export type PasswordLogoutRequest = z.infer<typeof PasswordLogoutRequest>;
export const PasswordChangePasswordRequest = z
    .object({
        currentPassword: z.string().nullable(),
        newPassword: z.string().nullable(),
    })
    .partial();
export type PasswordChangePasswordRequest = z.infer<
    typeof PasswordChangePasswordRequest
>;
export const TriggerJobRequest = z.object({
    jobKey: z.string().min(1),
    metadata: z.record(z.string(), z.string()).nullish(),
    delaySeconds: z.number().int().nullish(),
    executionMode: z.string().nullish(),
});
export type TriggerJobRequest = z.infer<typeof TriggerJobRequest>;
export const TriggerJobResponse = z
    .object({ status: z.string().nullable(), jobKey: z.string().nullable() })
    .partial();
export type TriggerJobResponse = z.infer<typeof TriggerJobResponse>;
export const WorkPollRequest = z.object({
    environmentTag: z.string().nullish(),
    runnerId: z.string().min(1),
    runnerInstanceId: z.string().nullish(),
    batchSize: z.number().int().nullish(),
    waitForMs: z.number().int().nullish(),
    allowTestExecutions: z.boolean().nullish(),
    maxInflight: z.number().int().nullish(),
    capabilities: z.array(z.string()).nullish(),
});
export type WorkPollRequest = z.infer<typeof WorkPollRequest>;
export const WorkLeaseToken = z.object({
    executionId: z.string().min(1),
    leaseId: z.string().min(1),
    triggerId: z.string().min(1),
    jobKey: z.string().min(1),
    fireAtUtc: z.iso.datetime({ offset: true }).optional(),
    leaseExpiresAtUtc: z.iso.datetime({ offset: true }).optional(),
    payload: z.string().nullish(),
    executionMode: z.string().nullish(),
    invocationSource: z.string().nullish(),
});
export type WorkLeaseToken = z.infer<typeof WorkLeaseToken>;
export const WorkPollResponse = z
    .object({ leases: z.array(WorkLeaseToken).nullable() })
    .partial();
export type WorkPollResponse = z.infer<typeof WorkPollResponse>;
export const WorkRenewRequest = z.object({
    environmentTag: z.string().nullish(),
    runnerId: z.string().min(1),
    lease: WorkLeaseToken,
});
export type WorkRenewRequest = z.infer<typeof WorkRenewRequest>;
export const WorkRenewResponse = z
    .object({ renewed: z.boolean(), lease: WorkLeaseToken })
    .partial();
export type WorkRenewResponse = z.infer<typeof WorkRenewResponse>;
export const WorkAckRequest = z.object({
    environmentTag: z.string().nullish(),
    runnerId: z.string().min(1),
    lease: WorkLeaseToken,
    succeeded: z.boolean().optional(),
    nextFireTimeUtc: z.iso.datetime({ offset: true }).nullish(),
    deadLetterReason: z.string().nullish(),
});
export type WorkAckRequest = z.infer<typeof WorkAckRequest>;
export const WorkEventEntry = z.object({
    message: z.string().min(1),
    level: z.string().nullish(),
    timestampUtc: z.iso.datetime({ offset: true }).nullish(),
    properties: z.record(z.string(), z.string()).nullish(),
    eventType: z.string().nullish(),
});
export type WorkEventEntry = z.infer<typeof WorkEventEntry>;
export const WorkEventsRequest = z.object({
    environmentTag: z.string().nullish(),
    runnerId: z.string().min(1),
    lease: WorkLeaseToken,
    events: z.array(WorkEventEntry).nullish(),
});
export type WorkEventsRequest = z.infer<typeof WorkEventsRequest>;
export const RunnerHeartbeatRequest = z.object({
    environmentTag: z.string().nullish(),
    runnerId: z.string().min(1),
    runnerInstanceId: z.string().nullish(),
    seenAtUtc: z.iso.datetime({ offset: true }).nullish(),
    metadataJson: z.string().nullish(),
});
export type RunnerHeartbeatRequest = z.infer<typeof RunnerHeartbeatRequest>;
export const RunnerStatusModel = z.object({
    runnerId: z.string().min(1),
    lastSeenAtUtc: z.iso.datetime({ offset: true }).optional(),
    expiresAtUtc: z.iso.datetime({ offset: true }).optional(),
    isOnline: z.boolean().optional(),
    metadataJson: z.string().nullish(),
});
export type RunnerStatusModel = z.infer<typeof RunnerStatusModel>;
export const RunnerListResponse = z
    .object({ runners: z.array(RunnerStatusModel).nullable() })
    .partial();
export type RunnerListResponse = z.infer<typeof RunnerListResponse>;
export const WorkerHeartbeatRequest = z.object({
    environmentTag: z.string().nullish(),
    instanceId: z.string().min(1),
    seenAtUtc: z.iso.datetime({ offset: true }).nullish(),
    metadataJson: z.string().nullish(),
});
export type WorkerHeartbeatRequest = z.infer<typeof WorkerHeartbeatRequest>;
export const WorkerStatusModel = z.object({
    instanceId: z.string().min(1),
    lastSeenAtUtc: z.iso.datetime({ offset: true }).optional(),
    expiresAtUtc: z.iso.datetime({ offset: true }).optional(),
    isOnline: z.boolean().optional(),
    metadataJson: z.string().nullish(),
});
export type WorkerStatusModel = z.infer<typeof WorkerStatusModel>;
export const WorkerListResponse = z
    .object({ workers: z.array(WorkerStatusModel).nullable() })
    .partial();
export type WorkerListResponse = z.infer<typeof WorkerListResponse>;
export const schemas = {
    HealthStatusResponse,
    PersistenceHealthResponse,
    CallerType,
    CallerInfoResponse,
    UpsertTenantRequest,
    TenantResponse,
    JobResponse,
    UpsertJobRequest,
    ExecutionKind,
    ExecutionStatus,
    ExecutionResponse,
    CroniqTriggerSeedDefinition,
    ScheduleUpsertResult,
    ScheduleResponse,
    CalendarMode,
    CalendarRuleType,
    CalendarDailyWindowRule,
    CalendarWeeklyWindowRule,
    CalendarAnnualDateListRule,
    CalendarDateListRule,
    CalendarCronRule,
    CalendarRuleDefinition,
    CalendarResponse,
    CroniqCalendarSeedDefinition,
    CalendarUpsertResult,
    ScheduleDeadLetterResponse,
    ScheduleReplayResult,
    ScheduleForecastBucket,
    ScheduleForecastSummary,
    ScheduleForecastResponse,
    WebhookIpRuleResponse,
    WebhookEndpointResponse,
    UpsertWebhookEndpointRequest,
    WebhookCapabilitiesResponse,
    WebhookRemoteHealthResponse,
    RotateWebhookSecretRequest,
    RotateWebhookSecretResponse,
    CreateWebhookIpRuleRequest,
    WebhookEndpointEventResponse,
    WebhookInvokeResult,
    WebhookDeadLetterResponse,
    WebhookActivityTimelineEntry,
    WebhookActivityBucket,
    WebhookActivitySummary,
    WebhookReplayResult,
    WebhookDeadLetterFailureRequest,
    ApiClientResponse,
    UpsertApiClientRequest,
    IssueApiKeyRequest,
    IssueApiKeyResponse,
    IssueTokenRequest,
    IssueTokenResponse,
    PasswordLoginRequest,
    PasswordAuthResponse,
    PasswordRefreshRequest,
    OidcAuthResponse,
    PasswordLogoutRequest,
    PasswordChangePasswordRequest,
    TriggerJobRequest,
    TriggerJobResponse,
    WorkPollRequest,
    WorkLeaseToken,
    WorkPollResponse,
    WorkRenewRequest,
    WorkRenewResponse,
    WorkAckRequest,
    WorkEventEntry,
    WorkEventsRequest,
    RunnerHeartbeatRequest,
    RunnerStatusModel,
    RunnerListResponse,
    WorkerHeartbeatRequest,
    WorkerStatusModel,
    WorkerListResponse,
};
export type HttpMethod =
    | 'get'
    | 'post'
    | 'put'
    | 'patch'
    | 'delete'
    | 'options'
    | 'head';
export type RequestFormat =
    | 'json'
    | 'binary'
    | 'form-data'
    | 'url-encoded'
    | 'multipart'
    | 'unknown';
export type ParameterLocation = 'Path' | 'Query' | 'Body' | 'Header';
export interface EndpointParameter<TSchema = unknown> {
    name: string;
    description?: string;
    type?: ParameterLocation;
    schema: TSchema;
}
export interface EndpointError<TSchema = unknown> {
    status: number | 'default';
    description?: string;
    schema: TSchema;
}
export interface EndpointDefinition<TResponse = unknown> {
    method: HttpMethod;
    path: string;
    description?: string;
    alias?: string;
    requestFormat?: RequestFormat;
    parameters?: EndpointParameter[];
    response: TResponse;
    errors?: EndpointError[];
}
export type EndpointList = ReadonlyArray<EndpointDefinition>;
