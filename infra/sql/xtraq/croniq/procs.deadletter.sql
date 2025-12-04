-- Procedures for trigger dead letter insert and retention
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerDeadLetterInsert]
    @Actor [core].[ActorRef] READONLY,
    @DeadLetter [croniq].[TriggerDeadLetterRef] READONLY,
    @DeadLetterId [core].[keyBig] OUTPUT,
    @CreatedUtc [core].[utcDateTime] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @ActorValue [core].[actor];
    DECLARE @TriggerId [core].[keyBig];
    DECLARE @TenantId [core].[key];
    DECLARE @Environment [core].[tag];
    DECLARE @Namespace [core].[label];
    DECLARE @Name [core].[name];
    DECLARE @Variant [croniq].[jobVariant];
    DECLARE @FireAtUtc [core].[utcDateTime];
    DECLARE @DeadLetterReason [croniq].[deadLetterReason];
    DECLARE @Payload [core].[jsonNullable];

    EXEC [core].[GuardActor] @Actor, @ActorValue OUTPUT;
    EXEC [croniq].[GuardTriggerDeadLetterRef] @DeadLetter, @TriggerId OUTPUT, @TenantId OUTPUT, @Environment OUTPUT, @Namespace OUTPUT, @Name OUTPUT, @Variant OUTPUT, @FireAtUtc OUTPUT, @DeadLetterReason OUTPUT, @Payload OUTPUT;

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

    SET @DeadLetterId = SCOPE_IDENTITY();

    SELECT @CreatedUtc = td.[CreatedUtc]
    FROM [croniq].[TriggerDeadLetter] AS td
    WHERE td.[DeadLetterId] = @DeadLetterId;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[TriggerDeadLetterRetention]
    @Actor [core].[ActorRef] READONLY,
    @RetentionDays [core].[number] = 30,
    @DeletedCount [core].[count] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    DECLARE @ActorValue [core].[actor];
    DECLARE @cutoff [core].[utcDateTime] = DATEADD(DAY, -@RetentionDays, SYSUTCDATETIME());

    EXEC [core].[GuardActor] @Actor, @ActorValue OUTPUT;

    DELETE FROM [croniq].[TriggerDeadLetter]
    WHERE [IsDeleted] = 1
      AND [CreatedUtc] <= @cutoff;

    SET @DeletedCount = @@ROWCOUNT;
END
GO
