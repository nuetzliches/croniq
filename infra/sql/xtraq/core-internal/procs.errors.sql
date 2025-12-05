SET QUOTED_IDENTIFIER ON;
SET ANSI_NULLS ON;
GO

CREATE OR ALTER PROCEDURE [core-internal].[ThrowActorRequired]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50004, 'Actor reference required', 1;
END
GO
