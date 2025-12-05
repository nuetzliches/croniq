-- Croniq schema domain types
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'croniq') EXEC ('CREATE SCHEMA [croniq]');
GO

CREATE TYPE [croniq].[jobVariant] FROM NVARCHAR(64) NULL;
GO

CREATE TYPE [croniq].[deadLetterReason] FROM NVARCHAR(128) NULL;
GO

CREATE TYPE [croniq].[cronExpression] FROM NVARCHAR(256) NOT NULL;
GO

CREATE TYPE [croniq].[timeZoneId] FROM NVARCHAR(64) NOT NULL;
GO

CREATE TYPE [croniq].[stateCode] FROM NVARCHAR(32) NOT NULL;
GO

CREATE TYPE [croniq].[InstanceRef] AS TABLE
(
    [InstanceId] [core].[reference],
    [Environment] [core].[tag],
    [NodeName] [core].[label],
    [Capabilities] [core].[jsonNullable],
    [Version] [core].[label]
);
GO

CREATE TYPE [croniq].[JobRef] AS TABLE
(
    [JobKey] [core].[reference],
    [TenantId] [core].[key],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [Description] [core].[labelNullable],
    [Metadata] [core].[jsonNullable]
);
GO

CREATE TYPE [croniq].[TriggerRef] AS TABLE
(
    [TriggerKey] [core].[reference],
    [JobKey] [core].[reference],
    [TenantId] [core].[key],
    [JobId] [core].[keyBig],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [CronExpression] [croniq].[cronExpression],
    [TimeZoneId] [croniq].[timeZoneId],
    [StartAtUtc] [core].[utcDateTimeNullable],
    [EndAtUtc] [core].[utcDateTimeNullable],
    [Enabled] [core].[flag],
    [Metadata] [core].[jsonNullable]
);
GO

CREATE TYPE [croniq].[TriggerLeaseRef] AS TABLE
(
    [TriggerId] [core].[keyBig],
    [JobId] [core].[keyBig],
    [TenantId] [core].[key],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [InstanceId] [core].[reference],
    [FireAtUtc] [core].[utcDateTime],
    [LeaseExpiresAtUtc] [core].[utcDateTime],
    [Payload] [core].[jsonNullable]
);
GO

CREATE TYPE [croniq].[TriggerLeaseReleaseRef] AS TABLE
(
    [LeaseId] [core].[keyBig],
    [InstanceId] [core].[reference]
);
GO

CREATE TYPE [croniq].[TriggerDeadLetterRef] AS TABLE
(
    [TriggerId] [core].[keyBig],
    [TenantId] [core].[key],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [FireAtUtc] [core].[utcDateTime],
    [DeadLetterReason] [croniq].[deadLetterReason],
    [Payload] [core].[jsonNullable]
);
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceId]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[reference]
AS
BEGIN
    DECLARE @Result [core].[reference];

    SELECT TOP (1) @Result = ir.[InstanceId]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[GuardInstanceRef]
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
        EXEC [croniq].[ThrowInstanceRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq].[GuardJobRef]
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
        EXEC [croniq].[ThrowJobRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq].[GuardTriggerRef]
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
        @Enabled = tr.[Enabled],
        @Metadata = tr.[Metadata]
    FROM @Trigger AS tr;

    IF @TriggerKey IS NULL OR @JobKey IS NULL OR @TenantId IS NULL OR @Environment IS NULL OR @Namespace IS NULL OR @Name IS NULL OR @Variant IS NULL OR @CronExpression IS NULL OR @TimeZoneId IS NULL OR @Enabled IS NULL
    BEGIN;
        EXEC [croniq].[ThrowTriggerRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq].[GuardTriggerLeaseRef]
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
        EXEC [croniq].[ThrowTriggerLeaseRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq].[GuardTriggerLeaseReleaseRef]
    @Release [croniq].[TriggerLeaseReleaseRef] READONLY,
    @LeaseId [core].[keyBig] OUTPUT,
    @InstanceId [core].[reference] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SELECT TOP (1)
        @LeaseId = rr.[LeaseId],
        @InstanceId = rr.[InstanceId]
    FROM @Release AS rr;

    IF @LeaseId IS NULL OR @InstanceId IS NULL
    BEGIN;
        EXEC [croniq].[ThrowTriggerLeaseReleaseRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq].[GuardTriggerDeadLetterRef]
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
        EXEC [croniq].[ThrowTriggerDeadLetterRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowInstanceRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50002, 'Instance reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowInstanceNotRegistered]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50001, 'Instance not registered', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowInstanceIdentityMismatch]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50003, 'Instance identity mismatch', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowInstanceReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50005, 'Instance reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowJobRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50006, 'Job reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowJobNotFound]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50007, 'Job not found', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowJobReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50008, 'Job reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50009, 'Trigger reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerNotFound]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50010, 'Trigger not found', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50011, 'Trigger reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerJobMissing]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50012, 'Referenced job not found or deleted', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerLeaseRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50013, 'Trigger lease reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerLeaseReleaseRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50014, 'Trigger lease release reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerLeaseActive]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50015, 'Trigger lease already active', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerLeaseNotFound]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50016, 'Trigger lease not found', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerLeaseOwnershipMismatch]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50017, 'Trigger lease ownership mismatch', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerLeaseReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50018, 'Trigger lease reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowTriggerDeadLetterRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50019, 'Trigger dead letter reference incomplete', 1;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceEnvironment]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[tag]
AS
BEGIN
    DECLARE @Result [core].[tag];

    SELECT TOP (1) @Result = ir.[Environment]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceNodeName]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[label]
AS
BEGIN
    DECLARE @Result [core].[label];

    SELECT TOP (1) @Result = ir.[NodeName]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceCapabilities]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[jsonNullable]
AS
BEGIN
    DECLARE @Result [core].[jsonNullable];

    SELECT TOP (1) @Result = ir.[Capabilities]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceVersion]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[labelNullable]
AS
BEGIN
    DECLARE @Result [core].[label];

    SELECT TOP (1) @Result = ir.[Version]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO
