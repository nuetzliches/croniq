SET QUOTED_IDENTIFIER ON;
SET ANSI_NULLS ON;
GO

-- Procedures for cluster instances: registration, heartbeat, lease cleanup
GO

CREATE OR ALTER PROCEDURE [croniq].[InstanceRegister]
    @Actor [core].[ActorRef] READONLY,
    @Instance [croniq].[InstanceRef] READONLY,
    @AllowDeletedReuse [core].[flag],
    @Generation [core].[count] OUTPUT,
    @LastSeenUtc [core].[utcDateTime] OUTPUT
AS
BEGIN

    DECLARE @ActorValue [core].[actor];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();
    DECLARE @InstanceId [core].[reference];
    DECLARE @Environment [core].[tag];
    DECLARE @NodeName [core].[label];
    DECLARE @Capabilities [core].[jsonNullable];
    DECLARE @Version [core].[label];
    DECLARE @ExistingEnvironment [core].[tag];
    DECLARE @ExistingNodeName [core].[label];
    DECLARE @ExistingIsDeleted [core].[flag];

    EXEC [core-internal].[GuardActor] @Actor, @ActorValue OUTPUT;
    EXEC [croniq-internal].[GuardInstanceRef] @Instance, @InstanceId OUTPUT, @Environment OUTPUT, @NodeName OUTPUT, @Capabilities OUTPUT, @Version OUTPUT;

    SELECT @ExistingEnvironment = i.[Environment],
        @ExistingNodeName = i.[NodeName],
        @ExistingIsDeleted = i.[IsDeleted]
    FROM [croniq].[Instances] AS i
    WITH (UPDLOCK, HOLDLOCK)
    WHERE i.[InstanceId] = @InstanceId;

    IF @ExistingEnvironment IS NOT NULL
    BEGIN
        IF @ExistingIsDeleted = 0 AND (@ExistingEnvironment != @Environment OR @ExistingNodeName != @NodeName)
        BEGIN;
            EXEC [croniq-internal].[ThrowInstanceIdentityMismatch];
        END
        ELSE IF @ExistingIsDeleted = 1 AND @AllowDeletedReuse = 0
        BEGIN;
            EXEC [croniq-internal].[ThrowInstanceReuseNotAllowed];
        END

        UPDATE [croniq].[Instances]
        SET [Environment] = @Environment,
            [NodeName] = @NodeName,
            [Capabilities] = @Capabilities,
            [Version] = @Version,
            [Generation] = [Generation] + 1,
            [StartedUtc] = @now,
            [LastSeenUtc] = @now,
            [UpdatedUtc] = @now,
            [UpdatedBy] = @ActorValue,
            [IsDeleted] = 0
        WHERE [InstanceId] = @InstanceId;
    END
    ELSE
    BEGIN
        INSERT INTO [croniq].[Instances]
        (
            [InstanceId],
            [Environment],
            [NodeName],
            [Capabilities],
            [Version],
            [Generation],
            [StartedUtc],
            [LastSeenUtc],
            [CreatedUtc],
            [CreatedBy],
            [IsDeleted]
        )
        VALUES
        (
            @InstanceId,
            @Environment,
            @NodeName,
            @Capabilities,
            @Version,
            1,
            @now,
            @now,
            @now,
            @ActorValue,
            0
        );
    END

    SELECT @Generation = i.[Generation],
        @LastSeenUtc = i.[LastSeenUtc]
    FROM [croniq].[Instances] AS i
    WHERE i.[InstanceId] = @InstanceId;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[InstanceHeartbeat]
    @Actor [core].[ActorRef] READONLY,
    @Instance [croniq].[InstanceRef] READONLY,
    @Generation [core].[count] OUTPUT,
    @LastSeenUtc [core].[utcDateTime] OUTPUT
AS
BEGIN

    DECLARE @ActorValue [core].[actor];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();
    DECLARE @InstanceId [core].[reference];
    DECLARE @Environment [core].[tag];
    DECLARE @NodeName [core].[label];
    DECLARE @Capabilities [core].[jsonNullable];
    DECLARE @Version [core].[label];

    EXEC [core-internal].[GuardActor] @Actor, @ActorValue OUTPUT;
    EXEC [croniq-internal].[GuardInstanceRef] @Instance, @InstanceId OUTPUT, @Environment OUTPUT, @NodeName OUTPUT, @Capabilities OUTPUT, @Version OUTPUT;

    IF NOT EXISTS (SELECT TOP 1 1 FROM [croniq].[Instances] WITH (UPDLOCK, HOLDLOCK) WHERE [InstanceId] = @InstanceId AND [IsDeleted] = 0)
    BEGIN;
        EXEC [croniq-internal].[ThrowInstanceNotRegistered];
    END

    IF NOT EXISTS (SELECT TOP 1 1 FROM [croniq].[Instances] WITH (UPDLOCK, HOLDLOCK) WHERE [InstanceId] = @InstanceId AND [Environment] = @Environment AND [NodeName] = @NodeName AND [IsDeleted] = 0)
    BEGIN;
        EXEC [croniq-internal].[ThrowInstanceIdentityMismatch];
    END

    UPDATE [croniq].[Instances]
    SET [LastSeenUtc] = @now,
        [Capabilities] = COALESCE(@Capabilities, [Capabilities]),
        [Version] = COALESCE(@Version, [Version]),
        [UpdatedUtc] = @now,
        [UpdatedBy] = @ActorValue
    WHERE [InstanceId] = @InstanceId
        AND [Environment] = @Environment
        AND [NodeName] = @NodeName
        AND [IsDeleted] = 0;

    SELECT @Generation = i.[Generation],
        @LastSeenUtc = i.[LastSeenUtc]
    FROM [croniq].[Instances] AS i
    WHERE i.[InstanceId] = @InstanceId;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerLeaseCleanup]
    @Actor [core].[ActorRef] READONLY,
    @FailoverGraceSeconds [core].[number] = 30,
    @ReleasedCount [core].[count] OUTPUT
AS
BEGIN

    DECLARE @ActorValue [core].[actor];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();
    DECLARE @cutoff [core].[utcDateTime] = DATEADD(SECOND, -@FailoverGraceSeconds, @now);

    EXEC [core-internal].[GuardActor] @Actor, @ActorValue OUTPUT;

    WITH stale AS
    (
        SELECT tl.[LeaseId]
        FROM [croniq].[TriggerLeases] AS tl
        WHERE tl.[IsDeleted] = 0
          AND
          (
              tl.[LeaseExpiresAtUtc] <= @now
                OR NOT EXISTS
                   (
                       SELECT TOP 1 1
                       FROM [croniq].[Instances] AS i
                       WHERE i.[InstanceId] = tl.[InstanceId]
                         AND i.[IsDeleted] = 0
                         AND i.[LastSeenUtc] > @cutoff
                   )
          )
    )
    UPDATE tl
    SET tl.[IsDeleted] = 1,
        tl.[UpdatedUtc] = @now,
        tl.[UpdatedBy] = @ActorValue
    FROM [croniq].[TriggerLeases] AS tl
        INNER JOIN stale AS s 
            ON tl.[LeaseId] = s.[LeaseId];

    SET @ReleasedCount = @@ROWCOUNT;
END
GO
