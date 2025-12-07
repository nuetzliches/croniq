SET QUOTED_IDENTIFIER ON;
SET ANSI_NULLS ON;
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[GuardInstanceRef]
    @Instance [croniq].[InstanceRef] READONLY,
    @InstanceId [core].[reference] OUTPUT,
    @Environment [core].[tag] OUTPUT,
    @NodeName [core].[label] OUTPUT,
    @Capabilities [core].[jsonNullable] OUTPUT,
    @Version [core].[label] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SELECT TOP (1)
        @InstanceId = ir.[InstanceId],
        @Environment = ir.[Environment],
        @NodeName = ir.[NodeName],
        @Capabilities = ir.[Capabilities],
        @Version = ir.[Version]
    FROM @Instance AS ir;

    IF @InstanceId IS NULL OR @Environment IS NULL OR @NodeName IS NULL
    BEGIN;
        EXEC [croniq-internal].[ThrowInstanceRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[GuardJobRef]
    @Job [croniq].[JobRef] READONLY,
    @JobKey [core].[reference] OUTPUT,
    @TenantId [core].[key] OUTPUT,
    @Environment [core].[tag] OUTPUT,
    @Namespace [core].[label] OUTPUT,
    @Name [core].[name] OUTPUT,
    @Variant [croniq].[jobVariant] OUTPUT,
    @Description [core].[labelNullable] OUTPUT,
    @Metadata [core].[jsonNullable] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SELECT TOP (1)
        @JobKey = jr.[JobKey],
        @TenantId = jr.[TenantId],
        @Environment = jr.[Environment],
        @Namespace = jr.[Namespace],
        @Name = jr.[Name],
        @Variant = jr.[Variant],
        @Description = jr.[Description],
        @Metadata = jr.[Metadata]
    FROM @Job AS jr;

    IF @JobKey IS NULL OR @TenantId IS NULL OR @Environment IS NULL OR @Namespace IS NULL OR @Name IS NULL
    BEGIN;
        EXEC [croniq-internal].[ThrowJobRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[GuardTriggerRef]
    @Trigger [croniq].[TriggerRef] READONLY,
    @TriggerKey [croniq].[triggerKey] OUTPUT,
    @JobKey [core].[reference] OUTPUT,
    @TenantId [core].[key] OUTPUT,
    @JobId [core].[keyBig] OUTPUT,
    @Environment [core].[tag] OUTPUT,
    @Namespace [core].[label] OUTPUT,
    @Name [core].[name] OUTPUT,
    @Variant [croniq].[jobVariant] OUTPUT,
    @CronExpression [croniq].[cronExpression] OUTPUT,
    @TimeZoneId [croniq].[timeZoneId] OUTPUT,
    @StartAtUtc [core].[utcDateTimeNullable] OUTPUT,
    @EndAtUtc [core].[utcDateTimeNullable] OUTPUT,
    @NextFireAtUtc [core].[utcDateTimeNullable] OUTPUT,
    @Enabled [core].[flag] OUTPUT,
    @Metadata [core].[jsonNullable] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SELECT TOP (1)
        @TriggerKey = tr.[TriggerKey],
        @JobKey = tr.[JobKey],
        @TenantId = tr.[TenantId],
        @JobId = tr.[JobId],
        @Environment = tr.[Environment],
        @Namespace = tr.[Namespace],
        @Name = tr.[Name],
        @Variant = tr.[Variant],
        @CronExpression = tr.[CronExpression],
        @TimeZoneId = tr.[TimeZoneId],
        @StartAtUtc = tr.[StartAtUtc],
        @EndAtUtc = tr.[EndAtUtc],
        @NextFireAtUtc = tr.[NextFireAtUtc],
        @Enabled = tr.[Enabled],
        @Metadata = tr.[Metadata]
    FROM @Trigger AS tr;

    IF @TriggerKey IS NULL OR @JobKey IS NULL OR @TenantId IS NULL OR @Environment IS NULL OR @Namespace IS NULL OR @Name IS NULL OR @CronExpression IS NULL OR @TimeZoneId IS NULL OR @Enabled IS NULL
    BEGIN;
        EXEC [croniq-internal].[ThrowTriggerRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[GuardTriggerLeaseRef]
    @Lease [croniq].[TriggerLeaseRef] READONLY,
    @TriggerId [core].[keyBig] OUTPUT,
    @JobId [core].[keyBig] OUTPUT,
    @TenantId [core].[key] OUTPUT,
    @Environment [core].[tag] OUTPUT,
    @Namespace [core].[label] OUTPUT,
    @Name [core].[name] OUTPUT,
    @Variant [croniq].[jobVariant] OUTPUT,
    @InstanceId [core].[reference] OUTPUT,
    @FireAtUtc [core].[utcDateTime] OUTPUT,
    @LeaseExpiresAtUtc [core].[utcDateTime] OUTPUT,
    @Payload [core].[jsonNullable] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SELECT TOP (1)
        @TriggerId = lr.[TriggerId],
        @JobId = lr.[JobId],
        @TenantId = lr.[TenantId],
        @Environment = lr.[Environment],
        @Namespace = lr.[Namespace],
        @Name = lr.[Name],
        @Variant = lr.[Variant],
        @InstanceId = lr.[InstanceId],
        @FireAtUtc = lr.[FireAtUtc],
        @LeaseExpiresAtUtc = lr.[LeaseExpiresAtUtc],
        @Payload = lr.[Payload]
    FROM @Lease AS lr;

    IF @TriggerId IS NULL OR @JobId IS NULL OR @TenantId IS NULL OR @Environment IS NULL OR @Namespace IS NULL OR @Name IS NULL OR @InstanceId IS NULL OR @FireAtUtc IS NULL OR @LeaseExpiresAtUtc IS NULL
    BEGIN;
        EXEC [croniq-internal].[ThrowTriggerLeaseRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[GuardTriggerLeaseReleaseRef]
    @Release [croniq].[TriggerLeaseReleaseRef] READONLY,
    @LeaseId [core].[keyBig] OUTPUT,
    @InstanceId [core].[reference] OUTPUT,
    @Succeeded [core].[flag] OUTPUT,
    @NextFireAtUtc [core].[utcDateTimeNullable] OUTPUT,
    @DeadLetterReason [croniq].[deadLetterReason] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SELECT TOP (1)
        @LeaseId = rr.[LeaseId],
        @InstanceId = rr.[InstanceId],
        @Succeeded = rr.[Succeeded],
        @NextFireAtUtc = rr.[NextFireAtUtc],
        @DeadLetterReason = rr.[DeadLetterReason]
    FROM @Release AS rr;

    IF @LeaseId IS NULL OR @InstanceId IS NULL OR @Succeeded IS NULL
    BEGIN;
        EXEC [croniq-internal].[ThrowTriggerLeaseReleaseRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[GuardTriggerDeadLetterRef]
    @DeadLetter [croniq].[TriggerDeadLetterRef] READONLY,
    @TriggerId [core].[keyBig] OUTPUT,
    @TenantId [core].[key] OUTPUT,
    @Environment [core].[tag] OUTPUT,
    @Namespace [core].[label] OUTPUT,
    @Name [core].[name] OUTPUT,
    @Variant [croniq].[jobVariant] OUTPUT,
    @FireAtUtc [core].[utcDateTime] OUTPUT,
    @DeadLetterReason [croniq].[deadLetterReason] OUTPUT,
    @Payload [core].[jsonNullable] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SELECT TOP (1)
        @TriggerId = dr.[TriggerId],
        @TenantId = dr.[TenantId],
        @Environment = dr.[Environment],
        @Namespace = dr.[Namespace],
        @Name = dr.[Name],
        @Variant = dr.[Variant],
        @FireAtUtc = dr.[FireAtUtc],
        @DeadLetterReason = dr.[DeadLetterReason],
        @Payload = dr.[Payload]
    FROM @DeadLetter AS dr;

    IF @TriggerId IS NULL OR @TenantId IS NULL OR @Environment IS NULL OR @Namespace IS NULL OR @Name IS NULL OR @FireAtUtc IS NULL OR @DeadLetterReason IS NULL
    BEGIN;
        EXEC [croniq-internal].[ThrowTriggerDeadLetterRefIncomplete];
    END
END
GO
