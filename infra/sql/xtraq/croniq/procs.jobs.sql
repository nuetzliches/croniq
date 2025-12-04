-- Procedures for Croniq jobs and triggers
GO

CREATE OR ALTER PROCEDURE [croniq].[JobUpsert]
    @Actor [core].[ActorRef] READONLY,
    @Job [croniq].[JobRef] READONLY,
    @AllowDeletedReuse [core].[flag] = 1,
    @JobId [core].[keyBig] OUTPUT,
    @UpdatedUtc [core].[utcDateTime] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @ActorValue [core].[actor];
    DECLARE @TenantId [core].[key];
    DECLARE @Environment [core].[tag];
    DECLARE @Namespace [core].[label];
    DECLARE @Name [core].[name];
    DECLARE @Variant [croniq].[jobVariant];
    DECLARE @Description [core].[labelNullable];
    DECLARE @Metadata [core].[jsonNullable];
    DECLARE @ExistingJobId [core].[keyBig];
    DECLARE @ExistingIsDeleted [core].[flag];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();

    EXEC [core].[GuardActor] @Actor, @ActorValue OUTPUT;
    EXEC [croniq].[GuardJobRef] @Job, @TenantId OUTPUT, @Environment OUTPUT, @Namespace OUTPUT, @Name OUTPUT, @Variant OUTPUT, @Description OUTPUT, @Metadata OUTPUT;

    SELECT @ExistingJobId = j.[JobId],
        @ExistingIsDeleted = j.[IsDeleted]
    FROM [croniq].[Jobs] AS j WITH (UPDLOCK, HOLDLOCK)
    WHERE j.[TenantId] = @TenantId
        AND j.[Environment] = @Environment
        AND j.[Namespace] = @Namespace
        AND j.[Name] = @Name
        AND j.[Variant] = @Variant;

    IF @ExistingJobId IS NOT NULL
    BEGIN
        IF @ExistingIsDeleted = 1 AND @AllowDeletedReuse = 0
        BEGIN;
            EXEC [croniq].[ThrowJobReuseNotAllowed];
        END

        UPDATE [croniq].[Jobs]
        SET [Description] = @Description,
            [Metadata] = @Metadata,
            [UpdatedUtc] = @now,
            [UpdatedBy] = @ActorValue,
            [IsDeleted] = 0
        WHERE [JobId] = @ExistingJobId;

        SET @JobId = @ExistingJobId;
    END
    ELSE
    BEGIN
        INSERT INTO [croniq].[Jobs]
        (
            [TenantId],
            [Environment],
            [Namespace],
            [Name],
            [Variant],
            [Description],
            [Metadata],
            [CreatedBy],
            [IsDeleted]
        )
        VALUES
        (
            @TenantId,
            @Environment,
            @Namespace,
            @Name,
            @Variant,
            @Description,
            @Metadata,
            @ActorValue,
            0
        );

        SET @JobId = SCOPE_IDENTITY();
    END

    SELECT TOP (1)
        @JobId = j.[JobId],
        @UpdatedUtc = COALESCE(j.[UpdatedUtc], j.[CreatedUtc])
    FROM [croniq].[Jobs] AS j
    WHERE j.[TenantId] = @TenantId
        AND j.[Environment] = @Environment
        AND j.[Namespace] = @Namespace
        AND j.[Name] = @Name
        AND j.[Variant] = @Variant;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[JobDelete]
    @Actor [core].[ActorRef] READONLY,
    @Job [croniq].[JobRef] READONLY,
    @JobId [core].[keyBig] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @ActorValue [core].[actor];
    DECLARE @TenantId [core].[key];
    DECLARE @Environment [core].[tag];
    DECLARE @Namespace [core].[label];
    DECLARE @Name [core].[name];
    DECLARE @Variant [croniq].[jobVariant];
    DECLARE @Description [core].[labelNullable];
    DECLARE @Metadata [core].[jsonNullable];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();

    EXEC [core].[GuardActor] @Actor, @ActorValue OUTPUT;
    EXEC [croniq].[GuardJobRef] @Job, @TenantId OUTPUT, @Environment OUTPUT, @Namespace OUTPUT, @Name OUTPUT, @Variant OUTPUT, @Description OUTPUT, @Metadata OUTPUT;

    SELECT TOP (1)
        @JobId = j.[JobId]
    FROM [croniq].[Jobs] AS j WITH (UPDLOCK, HOLDLOCK)
    WHERE j.[TenantId] = @TenantId
        AND j.[Environment] = @Environment
        AND j.[Namespace] = @Namespace
        AND j.[Name] = @Name
        AND j.[Variant] = @Variant
        AND j.[IsDeleted] = 0;

    IF @JobId IS NULL
    BEGIN;
        EXEC [croniq].[ThrowJobNotFound];
    END

    UPDATE [croniq].[Jobs]
    SET [IsDeleted] = 1,
        [UpdatedUtc] = @now,
        [UpdatedBy] = @ActorValue
    WHERE [JobId] = @JobId;

END
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerUpsert]
    @Actor [core].[ActorRef] READONLY,
    @Trigger [croniq].[TriggerRef] READONLY,
    @AllowDeletedReuse [core].[flag] = 1,
    @TriggerId [core].[keyBig] OUTPUT,
    @UpdatedUtc [core].[utcDateTime] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @ActorValue [core].[actor];
    DECLARE @TenantId [core].[key];
    DECLARE @JobId [core].[keyBig];
    DECLARE @Environment [core].[tag];
    DECLARE @Namespace [core].[label];
    DECLARE @Name [core].[name];
    DECLARE @Variant [croniq].[jobVariant];
    DECLARE @CronExpression [croniq].[cronExpression];
    DECLARE @TimeZoneId [croniq].[timeZoneId];
    DECLARE @StartAtUtc [core].[utcDateTimeNullable];
    DECLARE @EndAtUtc [core].[utcDateTimeNullable];
    DECLARE @Enabled [core].[flag];
    DECLARE @Metadata [core].[jsonNullable];
    DECLARE @ExistingTriggerId [core].[keyBig];
    DECLARE @ExistingIsDeleted [core].[flag];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();

    EXEC [core].[GuardActor] @Actor, @ActorValue OUTPUT;
    EXEC [croniq].[GuardTriggerRef] @Trigger, @TenantId OUTPUT, @JobId OUTPUT, @Environment OUTPUT, @Namespace OUTPUT, @Name OUTPUT, @Variant OUTPUT, @CronExpression OUTPUT, @TimeZoneId OUTPUT, @StartAtUtc OUTPUT, @EndAtUtc OUTPUT, @Enabled OUTPUT, @Metadata OUTPUT;

    IF NOT EXISTS (SELECT TOP (1) 1 FROM [croniq].[Jobs] WHERE [JobId] = @JobId AND [IsDeleted] = 0)
    BEGIN;
        EXEC [croniq].[ThrowTriggerJobMissing];
    END

    SELECT @ExistingTriggerId = t.[TriggerId],
        @ExistingIsDeleted = t.[IsDeleted]
    FROM [croniq].[Triggers] AS t WITH (UPDLOCK, HOLDLOCK)
    WHERE t.[TenantId] = @TenantId
        AND t.[Environment] = @Environment
        AND t.[Namespace] = @Namespace
        AND t.[Name] = @Name
        AND t.[Variant] = @Variant;

    IF @ExistingTriggerId IS NOT NULL
    BEGIN
        IF @ExistingIsDeleted = 1 AND @AllowDeletedReuse = 0
        BEGIN;
            EXEC [croniq].[ThrowTriggerReuseNotAllowed];
        END

        UPDATE [croniq].[Triggers]
        SET [JobId] = @JobId,
            [CronExpression] = @CronExpression,
            [TimeZoneId] = @TimeZoneId,
            [StartAtUtc] = @StartAtUtc,
            [EndAtUtc] = @EndAtUtc,
            [Enabled] = @Enabled,
            [Metadata] = @Metadata,
            [UpdatedUtc] = @now,
            [UpdatedBy] = @ActorValue,
            [IsDeleted] = 0
        WHERE [TriggerId] = @ExistingTriggerId;

        SET @TriggerId = @ExistingTriggerId;
    END
    ELSE
    BEGIN
        INSERT INTO [croniq].[Triggers]
        (
            [JobId],
            [TenantId],
            [Environment],
            [Namespace],
            [Name],
            [Variant],
            [CronExpression],
            [TimeZoneId],
            [StartAtUtc],
            [EndAtUtc],
            [Enabled],
            [Metadata],
            [CreatedBy],
            [IsDeleted]
        )
        VALUES
        (
            @JobId,
            @TenantId,
            @Environment,
            @Namespace,
            @Name,
            @Variant,
            @CronExpression,
            @TimeZoneId,
            @StartAtUtc,
            @EndAtUtc,
            @Enabled,
            @Metadata,
            @ActorValue,
            0
        );

        SET @TriggerId = SCOPE_IDENTITY();
    END

    SELECT TOP (1)
        @TriggerId = t.[TriggerId],
        @UpdatedUtc = COALESCE(t.[UpdatedUtc], t.[CreatedUtc])
    FROM [croniq].[Triggers] AS t
    WHERE t.[TenantId] = @TenantId
        AND t.[Environment] = @Environment
        AND t.[Namespace] = @Namespace
        AND t.[Name] = @Name
        AND t.[Variant] = @Variant;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerDelete]
    @Actor [core].[ActorRef] READONLY,
    @Trigger [croniq].[TriggerRef] READONLY,
    @TriggerId [core].[keyBig] OUTPUT,
    @UpdatedUtc [core].[utcDateTime] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @ActorValue [core].[actor];
    DECLARE @TenantId [core].[key];
    DECLARE @JobId [core].[keyBig];
    DECLARE @Environment [core].[tag];
    DECLARE @Namespace [core].[label];
    DECLARE @Name [core].[name];
    DECLARE @Variant [croniq].[jobVariant];
    DECLARE @CronExpression [croniq].[cronExpression];
    DECLARE @TimeZoneId [croniq].[timeZoneId];
    DECLARE @StartAtUtc [core].[utcDateTimeNullable];
    DECLARE @EndAtUtc [core].[utcDateTimeNullable];
    DECLARE @Enabled [core].[flag];
    DECLARE @Metadata [core].[jsonNullable];
    DECLARE @now [core].[utcDateTime] = SYSUTCDATETIME();

    EXEC [core].[GuardActor] @Actor, @ActorValue OUTPUT;
    EXEC [croniq].[GuardTriggerRef] @Trigger, @TenantId OUTPUT, @JobId OUTPUT, @Environment OUTPUT, @Namespace OUTPUT, @Name OUTPUT, @Variant OUTPUT, @CronExpression OUTPUT, @TimeZoneId OUTPUT, @StartAtUtc OUTPUT, @EndAtUtc OUTPUT, @Enabled OUTPUT, @Metadata OUTPUT;

    SELECT TOP (1)
        @TriggerId = t.[TriggerId]
    FROM [croniq].[Triggers] AS t WITH (UPDLOCK, HOLDLOCK)
    WHERE t.[TenantId] = @TenantId
        AND t.[Environment] = @Environment
        AND t.[Namespace] = @Namespace
        AND t.[Name] = @Name
        AND t.[Variant] = @Variant
        AND t.[IsDeleted] = 0;

    IF @TriggerId IS NULL
    BEGIN;
        EXEC [croniq].[ThrowTriggerNotFound];
    END

    UPDATE [croniq].[Triggers]
    SET [IsDeleted] = 1,
        [UpdatedUtc] = @now,
        [UpdatedBy] = @ActorValue
    WHERE [TriggerId] = @TriggerId;

    SELECT @UpdatedUtc = t.[UpdatedUtc]
    FROM [croniq].[Triggers] AS t
    WHERE t.[TriggerId] = @TriggerId;
END
GO
