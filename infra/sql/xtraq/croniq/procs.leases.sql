-- Procedures for trigger lease acquire, release, retention
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerLeaseAcquire]
    @Actor [core].[ActorRef] READONLY,
    @Lease [croniq].[TriggerLeaseRef] READONLY,
    @AllowDeletedReuse [core].[flag],
    @LeaseId [core].[keyBig] OUTPUT,
    @LeaseExpiresAtUtc [core].[utcDateTime] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @ActorValue [core].[actor];
    DECLARE @TriggerId [core].[keyBig];
    DECLARE @JobId [core].[keyBig];
    DECLARE @TenantId [core].[key];
    DECLARE @Environment [core].[tag];
    DECLARE @Namespace [core].[label];
    DECLARE @Name [core].[name];
    DECLARE @Variant [croniq].[jobVariant];
    DECLARE @InstanceId [core].[reference];
    DECLARE @FireAtUtc [core].[utcDateTime];
    DECLARE @RequestedLeaseExpiresAtUtc [core].[utcDateTime];
    DECLARE @Payload [core].[jsonNullable];
    DECLARE @ExistingLeaseId [core].[keyBig];
    DECLARE @ExistingExpiresAt [core].[utcDateTime];
    DECLARE @ExistingInstanceId [core].[reference];
    DECLARE @ExistingIsDeleted [core].[flag];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();

    EXEC [core].[GuardActor] @Actor, @ActorValue OUTPUT;
    EXEC [croniq].[GuardTriggerLeaseRef] @Lease, @TriggerId OUTPUT, @JobId OUTPUT, @TenantId OUTPUT, @Environment OUTPUT, @Namespace OUTPUT, @Name OUTPUT, @Variant OUTPUT, @InstanceId OUTPUT, @FireAtUtc OUTPUT, @RequestedLeaseExpiresAtUtc OUTPUT, @Payload OUTPUT;

    IF NOT EXISTS (SELECT TOP 1 1 FROM [croniq].[Triggers] WHERE [TriggerId] = @TriggerId AND [IsDeleted] = 0)
    BEGIN;
        EXEC [croniq].[ThrowTriggerNotFound];
    END

    SELECT TOP (1)
        @ExistingLeaseId = tl.[LeaseId],
        @ExistingExpiresAt = tl.[LeaseExpiresAtUtc],
        @ExistingInstanceId = tl.[InstanceId],
        @ExistingIsDeleted = tl.[IsDeleted]
    FROM [croniq].[TriggerLeases] AS tl WITH (UPDLOCK, HOLDLOCK)
    WHERE tl.[TriggerId] = @TriggerId
      AND tl.[IsDeleted] = 0
    ORDER BY tl.[LeaseExpiresAtUtc] DESC;

    IF @ExistingLeaseId IS NOT NULL AND @ExistingIsDeleted = 0 AND @ExistingExpiresAt > @now
    BEGIN;
        EXEC [croniq].[ThrowTriggerLeaseActive];
    END

    IF @ExistingLeaseId IS NOT NULL AND @ExistingIsDeleted = 1 AND @AllowDeletedReuse = 0
    BEGIN;
        EXEC [croniq].[ThrowTriggerLeaseReuseNotAllowed];
    END

    IF @ExistingLeaseId IS NOT NULL
    BEGIN
        UPDATE [croniq].[TriggerLeases]
        SET [JobId] = @JobId,
            [TenantId] = @TenantId,
            [Environment] = @Environment,
            [Namespace] = @Namespace,
            [Name] = @Name,
            [Variant] = @Variant,
            [InstanceId] = @InstanceId,
            [FireAtUtc] = @FireAtUtc,
            [LeaseExpiresAtUtc] = @RequestedLeaseExpiresAtUtc,
            [Payload] = @Payload,
            [UpdatedUtc] = @now,
            [UpdatedBy] = @ActorValue,
            [IsDeleted] = 0
        WHERE [LeaseId] = @ExistingLeaseId;

        SET @LeaseId = @ExistingLeaseId;
        SET @LeaseExpiresAtUtc = @RequestedLeaseExpiresAtUtc;
    END
    ELSE
    BEGIN
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
        VALUES
        (
            @TriggerId,
            @JobId,
            @TenantId,
            @Environment,
            @Namespace,
            @Name,
            @Variant,
            @InstanceId,
            @FireAtUtc,
            @RequestedLeaseExpiresAtUtc,
            @Payload,
            @ActorValue,
            0
        );

        SET @LeaseId = SCOPE_IDENTITY();
        SET @LeaseExpiresAtUtc = @RequestedLeaseExpiresAtUtc;
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerLeaseRelease]
    @Actor [core].[ActorRef] READONLY,
    @Release [croniq].[TriggerLeaseReleaseRef] READONLY,
    @ReleasedCount [core].[count] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @ActorValue [core].[actor];
    DECLARE @LeaseId [core].[keyBig];
    DECLARE @InstanceId [core].[reference];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();
    DECLARE @ExistingInstanceId [core].[reference];

    EXEC [core].[GuardActor] @Actor, @ActorValue OUTPUT;
    EXEC [croniq].[GuardTriggerLeaseReleaseRef] @Release, @LeaseId OUTPUT, @InstanceId OUTPUT;

    SELECT TOP (1)
        @ExistingInstanceId = tl.[InstanceId]
    FROM [croniq].[TriggerLeases] AS tl WITH (UPDLOCK, HOLDLOCK)
    WHERE tl.[LeaseId] = @LeaseId
      AND tl.[IsDeleted] = 0;

    IF @ExistingInstanceId IS NULL
    BEGIN;
        EXEC [croniq].[ThrowTriggerLeaseNotFound];
    END

    IF @ExistingInstanceId != @InstanceId
    BEGIN;
        EXEC [croniq].[ThrowTriggerLeaseOwnershipMismatch];
    END

    UPDATE [croniq].[TriggerLeases]
    SET [IsDeleted] = 1,
        [UpdatedUtc] = @now,
        [UpdatedBy] = @ActorValue
    WHERE [LeaseId] = @LeaseId
      AND [IsDeleted] = 0;

    SET @ReleasedCount = @@ROWCOUNT;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerLeaseRetention]
    @Actor [core].[ActorRef] READONLY,
    @RetentionDays [core].[number] = 30,
    @DeletedCount [core].[count] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @ActorValue [core].[actor];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();
    DECLARE @cutoff [core].[utcDateTime] = DATEADD(DAY, -@RetentionDays, @now);

    EXEC [core].[GuardActor] @Actor, @ActorValue OUTPUT;

    DELETE FROM [croniq].[TriggerLeases]
    WHERE [IsDeleted] = 1
      AND COALESCE([UpdatedUtc], [CreatedUtc]) <= @cutoff;

    SET @DeletedCount = @@ROWCOUNT;
END
GO
