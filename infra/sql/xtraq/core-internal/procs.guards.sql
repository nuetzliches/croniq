SET QUOTED_IDENTIFIER ON;
SET ANSI_NULLS ON;
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
