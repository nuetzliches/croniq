-- Core-internal schema for guard/throw routines
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'core-internal') EXEC ('CREATE SCHEMA [core-internal]');
GO

CREATE OR ALTER PROCEDURE [core-internal].[ThrowActorRequired]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50004, 'Actor reference required', 1;
END
GO

CREATE OR ALTER PROCEDURE [core-internal].[GuardActor]
    @Actor [core].[ActorRef] READONLY,
    @ActorValue [core].[actor] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SET @ActorValue = [core].[GetActor](@Actor);

    IF @ActorValue IS NULL
    BEGIN;
        EXEC [core-internal].[ThrowActorRequired];
    END
END
GO
