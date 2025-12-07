SET QUOTED_IDENTIFIER ON;
SET ANSI_NULLS ON;
GO

-- Procedures for trigger lease acquire, release, retention
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerLeaseAcquire]
    @Actor [core].[ActorRef] READONLY,
    @TenantId [core].[key],
    @Environment [core].[tag],
    @InstanceId [core].[reference],
    @NowUtc [core].[utcDateTime],
    @BatchSize [core].[count] = 10,
    @LeaseDurationSeconds [core].[number] = 30
AS
BEGIN

    DECLARE @ActorValue [core].[actor];
    DECLARE @LeaseExpiresAtUtc [core].[utcDateTime] = DATEADD(SECOND, @LeaseDurationSeconds, @NowUtc);

    EXEC [core-internal].[GuardActor] @Actor, @ActorValue OUTPUT;

    DECLARE @due TABLE
    (
        [RowId] INT IDENTITY(1,1) PRIMARY KEY,
        [TriggerId] [core].[keyBig],
        [TriggerKey] [croniq].[triggerKey],
        [JobId] [core].[keyBig],
        [JobKey] [core].[reference],
        [TenantId] [core].[key],
        [Environment] [core].[tag],
        [Namespace] [core].[label],
        [Name] [core].[name],
        [Variant] [croniq].[jobVariant],
        [CronExpression] [croniq].[cronExpression],
        [TimeZoneId] [croniq].[timeZoneId],
        [StartAtUtc] [core].[utcDateTimeNullable],
        [EndAtUtc] [core].[utcDateTimeNullable],
        [Metadata] [core].[jsonNullable],
        [FireAtUtc] [core].[utcDateTime],
        [ExistingLeaseId] [core].[keyBig] NULL,
        [ExistingIsDeleted] [core].[flag] NULL,
        [ExistingExpiresAtUtc] [core].[utcDateTimeNullable]
    );

    INSERT INTO @due
    (
        [TriggerId],
        [TriggerKey],
        [JobId],
        [JobKey],
        [TenantId],
        [Environment],
        [Namespace],
        [Name],
        [Variant],
        [CronExpression],
        [TimeZoneId],
        [StartAtUtc],
        [EndAtUtc],
        [Metadata],
        [FireAtUtc],
        [ExistingLeaseId],
        [ExistingIsDeleted],
        [ExistingExpiresAtUtc]
    )
    SELECT TOP (@BatchSize)
        t.[TriggerId],
        t.[TriggerKey],
        t.[JobId],
        t.[JobKey],
        t.[TenantId],
        t.[Environment],
        t.[Namespace],
        t.[Name],
        t.[Variant],
        t.[CronExpression],
        t.[TimeZoneId],
        t.[StartAtUtc],
        t.[EndAtUtc],
        t.[Metadata],
        t.[NextFireAtUtc],
        l.[LeaseId],
        l.[IsDeleted],
        l.[LeaseExpiresAtUtc]
    FROM [croniq].[Triggers] AS t WITH (UPDLOCK, READPAST, ROWLOCK)
        OUTER APPLY
        (
            SELECT TOP (1)
                tl.[LeaseId],
                tl.[IsDeleted],
                tl.[LeaseExpiresAtUtc]
            FROM [croniq].[TriggerLeases] AS tl WITH (UPDLOCK, READPAST)
            WHERE tl.[TriggerId] = t.[TriggerId]
            ORDER BY tl.[LeaseId] DESC
        ) AS l
    WHERE t.[TenantId] = @TenantId
      AND t.[Environment] = @Environment
      AND t.[IsDeleted] = 0
      AND t.[Enabled] = 1
      AND t.[NextFireAtUtc] IS NOT NULL
      AND t.[NextFireAtUtc] <= @NowUtc
      AND (l.[LeaseId] IS NULL OR l.[IsDeleted] = 1 OR l.[LeaseExpiresAtUtc] <= @NowUtc)
    ORDER BY t.[NextFireAtUtc], t.[TriggerId];

    DECLARE @acquired TABLE
    (
        [LeaseId] [core].[keyBig],
        [TriggerId] [core].[keyBig],
        [TriggerKey] [croniq].[triggerKey] NULL,
        [JobId] [core].[keyBig],
        [JobKey] [core].[reference] NULL,
        [TenantId] [core].[key],
        [Environment] [core].[tag],
        [Namespace] [core].[label],
        [Name] [core].[name],
        [Variant] [croniq].[jobVariant],
        [CronExpression] [croniq].[cronExpression] NULL,
        [TimeZoneId] [croniq].[timeZoneId] NULL,
        [StartAtUtc] [core].[utcDateTimeNullable],
        [EndAtUtc] [core].[utcDateTimeNullable],
        [Metadata] [core].[jsonNullable],
        [InstanceId] [core].[reference],
        [FireAtUtc] [core].[utcDateTime],
        [LeaseExpiresAtUtc] [core].[utcDateTime],
        [Payload] [core].[jsonNullable]
    );

    -- Refresh existing lease rows (expired or soft-deleted)
    UPDATE tl
    SET [JobId] = d.[JobId],
        [TenantId] = d.[TenantId],
        [Environment] = d.[Environment],
        [Namespace] = d.[Namespace],
        [Name] = d.[Name],
        [Variant] = d.[Variant],
        [InstanceId] = @InstanceId,
        [FireAtUtc] = d.[FireAtUtc],
        [LeaseExpiresAtUtc] = @LeaseExpiresAtUtc,
        [Payload] = d.[Metadata],
        [UpdatedUtc] = @NowUtc,
        [UpdatedBy] = @ActorValue,
        [IsDeleted] = 0
    OUTPUT inserted.[LeaseId],
        inserted.[TriggerId],
        inserted.[JobId],
        inserted.[TenantId],
        inserted.[Environment],
        inserted.[Namespace],
        inserted.[Name],
        inserted.[Variant],
        inserted.[InstanceId],
        inserted.[FireAtUtc],
        inserted.[LeaseExpiresAtUtc],
        inserted.[Payload]
    INTO @acquired ([LeaseId], [TriggerId], [JobId], [TenantId], [Environment], [Namespace], [Name], [Variant], [InstanceId], [FireAtUtc], [LeaseExpiresAtUtc], [Payload])
    FROM [croniq].[TriggerLeases] AS tl
        INNER JOIN @due AS d
            ON d.[ExistingLeaseId] = tl.[LeaseId];

    -- Insert fresh leases
    INSERT INTO [croniq].[TriggerLeases]
    (
        [TriggerId],
        [JobId],
        [TenantId],
        [Environment],
        [Namespace],
        [Name],
        [Variant],
        [InstanceId],
        [FireAtUtc],
        [LeaseExpiresAtUtc],
        [Payload],
        [CreatedBy],
        [IsDeleted]
    )
    OUTPUT inserted.[LeaseId],
        inserted.[TriggerId],
        inserted.[JobId],
        inserted.[TenantId],
        inserted.[Environment],
        inserted.[Namespace],
        inserted.[Name],
        inserted.[Variant],
        inserted.[InstanceId],
        inserted.[FireAtUtc],
        inserted.[LeaseExpiresAtUtc],
        inserted.[Payload]
    INTO @acquired ([LeaseId], [TriggerId], [JobId], [TenantId], [Environment], [Namespace], [Name], [Variant], [InstanceId], [FireAtUtc], [LeaseExpiresAtUtc], [Payload])
    SELECT
        d.[TriggerId],
        d.[JobId],
        d.[TenantId],
        d.[Environment],
        d.[Namespace],
        d.[Name],
        d.[Variant],
        @InstanceId,
        d.[FireAtUtc],
        @LeaseExpiresAtUtc,
        d.[Metadata],
        @ActorValue,
        0
    FROM @due AS d
    WHERE d.[ExistingLeaseId] IS NULL;

    UPDATE a
    SET a.[TriggerKey] = d.[TriggerKey],
        a.[JobKey] = d.[JobKey],
        a.[CronExpression] = d.[CronExpression],
        a.[TimeZoneId] = d.[TimeZoneId],
        a.[StartAtUtc] = d.[StartAtUtc],
        a.[EndAtUtc] = d.[EndAtUtc],
        a.[Metadata] = d.[Metadata]
    FROM @acquired AS a
        INNER JOIN @due AS d
            ON a.[TriggerId] = d.[TriggerId];

    SELECT
        [LeaseId],
        [TriggerId],
        [TriggerKey],
        [JobId],
        [JobKey],
        [TenantId],
        [Environment],
        [Namespace],
        [Name],
        [Variant],
        [CronExpression],
        [TimeZoneId],
        [StartAtUtc],
        [EndAtUtc],
        [Metadata],
        [InstanceId],
        [FireAtUtc],
        [LeaseExpiresAtUtc],
        [Payload]
    FROM @acquired
    ORDER BY [FireAtUtc], [LeaseId];
END
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerLeaseRelease]
    @Actor [core].[ActorRef] READONLY,
    @Release [croniq].[TriggerLeaseReleaseRef] READONLY,
    @ReleasedCount [core].[count] OUTPUT
AS
BEGIN

    DECLARE @ActorValue [core].[actor];
    DECLARE @LeaseId [core].[keyBig];
    DECLARE @InstanceId [core].[reference];
    DECLARE @Succeeded [core].[flag];
    DECLARE @NextFireAtUtc [core].[utcDateTimeNullable];
    DECLARE @DeadLetterReason [croniq].[deadLetterReason];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();
    DECLARE @TriggerId [core].[keyBig];
    DECLARE @TenantId [core].[key];
    DECLARE @Environment [core].[tag];
    DECLARE @Namespace [core].[label];
    DECLARE @Name [core].[name];
    DECLARE @Variant [croniq].[jobVariant];
    DECLARE @FireAtUtc [core].[utcDateTime];
    DECLARE @Payload [core].[jsonNullable];
    DECLARE @ExistingInstanceId [core].[reference];

    EXEC [core-internal].[GuardActor] @Actor, @ActorValue OUTPUT;
    EXEC [croniq-internal].[GuardTriggerLeaseReleaseRef] @Release, @LeaseId OUTPUT, @InstanceId OUTPUT, @Succeeded OUTPUT, @NextFireAtUtc OUTPUT, @DeadLetterReason OUTPUT;

    SELECT TOP (1)
        @ExistingInstanceId = tl.[InstanceId],
        @TriggerId = tl.[TriggerId],
        @TenantId = tl.[TenantId],
        @Environment = tl.[Environment],
        @Namespace = tl.[Namespace],
        @Name = tl.[Name],
        @Variant = tl.[Variant],
        @FireAtUtc = tl.[FireAtUtc],
        @Payload = tl.[Payload]
    FROM [croniq].[TriggerLeases] AS tl WITH (UPDLOCK, HOLDLOCK)
    WHERE tl.[LeaseId] = @LeaseId
      AND tl.[IsDeleted] = 0;

    IF @ExistingInstanceId IS NULL
    BEGIN;
        EXEC [croniq-internal].[ThrowTriggerLeaseNotFound];
    END

    IF @ExistingInstanceId != @InstanceId
    BEGIN;
        EXEC [croniq-internal].[ThrowTriggerLeaseOwnershipMismatch];
    END

    UPDATE [croniq].[TriggerLeases]
    SET [IsDeleted] = 1,
        [UpdatedUtc] = @now,
        [UpdatedBy] = @ActorValue
    WHERE [LeaseId] = @LeaseId
      AND [IsDeleted] = 0;

    SET @ReleasedCount = @@ROWCOUNT;

    IF @ReleasedCount = 0
    BEGIN
        RETURN;
    END

    UPDATE [croniq].[Triggers]
    SET [LastFireAtUtc] = @FireAtUtc,
        [NextFireAtUtc] = @NextFireAtUtc,
        [UpdatedUtc] = @now,
        [UpdatedBy] = @ActorValue
    WHERE [TriggerId] = @TriggerId;

    IF @Succeeded = 0 AND @DeadLetterReason IS NOT NULL
    BEGIN
        INSERT INTO [croniq].[TriggerDeadLetter]
        (
            [TriggerId],
            [TenantId],
            [Environment],
            [Namespace],
            [Name],
            [Variant],
            [FireAtUtc],
            [DeadLetterReason],
            [Payload],
            [CreatedBy],
            [IsDeleted]
        )
        VALUES
        (
            @TriggerId,
            @TenantId,
            @Environment,
            @Namespace,
            @Name,
            @Variant,
            @FireAtUtc,
            @DeadLetterReason,
            @Payload,
            @ActorValue,
            0
        );
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerLeaseRetention]
    @Actor [core].[ActorRef] READONLY,
    @RetentionDays [core].[number] = 30,
    @DeletedCount [core].[count] OUTPUT
AS
BEGIN

    DECLARE @ActorValue [core].[actor];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();
    DECLARE @cutoff [core].[utcDateTime] = DATEADD(DAY, -@RetentionDays, @now);

    EXEC [core-internal].[GuardActor] @Actor, @ActorValue OUTPUT;

    DELETE FROM [croniq].[TriggerLeases]
    WHERE [IsDeleted] = 1
      AND COALESCE([UpdatedUtc], [CreatedUtc]) <= @cutoff;

    SET @DeletedCount = @@ROWCOUNT;
END
GO
