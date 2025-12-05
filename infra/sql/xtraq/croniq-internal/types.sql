-- Croniq-internal schema for guard/throw helpers
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'croniq-internal') EXEC ('CREATE SCHEMA [croniq-internal]');
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowInstanceRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50002, 'Instance reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowInstanceNotRegistered]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50001, 'Instance not registered', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowInstanceIdentityMismatch]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50003, 'Instance identity mismatch', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowInstanceReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50005, 'Instance reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowJobRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50006, 'Job reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowJobNotFound]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50007, 'Job not found', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowJobReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50008, 'Job reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50009, 'Trigger reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerNotFound]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50010, 'Trigger not found', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50011, 'Trigger reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerJobMissing]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50012, 'Referenced job not found or deleted', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50013, 'Trigger lease reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseReleaseRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50014, 'Trigger lease release reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseActive]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50015, 'Trigger lease already active', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseNotFound]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50016, 'Trigger lease not found', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseOwnershipMismatch]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50017, 'Trigger lease ownership mismatch', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50018, 'Trigger lease reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerDeadLetterRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50019, 'Trigger dead letter reference incomplete', 1;
END
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

    IF @JobKey IS NULL OR @TenantId IS NULL OR @Environment IS NULL OR @Namespace IS NULL OR @Name IS NULL OR @Variant IS NULL
    BEGIN;
        EXEC [croniq-internal].[ThrowJobRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[GuardTriggerRef]
    @Trigger [croniq].[TriggerRef] READONLY,
    @TriggerKey [core].[reference] OUTPUT,
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

    IF @TriggerKey IS NULL OR @JobKey IS NULL OR @TenantId IS NULL OR @Environment IS NULL OR @Namespace IS NULL OR @Name IS NULL OR @Variant IS NULL OR @CronExpression IS NULL OR @TimeZoneId IS NULL OR @Enabled IS NULL
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

    IF @TriggerId IS NULL OR @JobId IS NULL OR @TenantId IS NULL OR @Environment IS NULL OR @Namespace IS NULL OR @Name IS NULL OR @Variant IS NULL OR @InstanceId IS NULL OR @FireAtUtc IS NULL OR @LeaseExpiresAtUtc IS NULL
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

    IF @TriggerId IS NULL OR @TenantId IS NULL OR @Environment IS NULL OR @Namespace IS NULL OR @Name IS NULL OR @Variant IS NULL OR @FireAtUtc IS NULL OR @DeadLetterReason IS NULL
    BEGIN;
        EXEC [croniq-internal].[ThrowTriggerDeadLetterRefIncomplete];
    END
END
GO
