-- Core schema and user-defined types
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'core') EXEC ('CREATE SCHEMA [core]');
GO

-- primitives (NOT NULL)
CREATE TYPE [core].[key] FROM INT NOT NULL;
GO

CREATE TYPE [core].[keyBig] FROM BIGINT NOT NULL;
GO

CREATE TYPE [core].[uid] FROM UNIQUEIDENTIFIER NOT NULL;
GO

CREATE TYPE [core].[reference] FROM NVARCHAR(64) NOT NULL;
GO

CREATE TYPE [core].[tag] FROM NVARCHAR(32) NOT NULL;
GO

CREATE TYPE [core].[label] FROM NVARCHAR(64) NOT NULL;
GO

CREATE TYPE [core].[name] FROM NVARCHAR(128) NOT NULL;
GO

CREATE TYPE [core].[actor] FROM NVARCHAR(128) NOT NULL;
GO

CREATE TYPE [core].[utcDateTime] FROM DATETIME2(3) NOT NULL;
GO

CREATE TYPE [core].[utcDateTimeNullable] FROM DATETIME2(3) NULL;
GO

CREATE TYPE [core].[count] FROM INT NOT NULL;
GO

CREATE TYPE [core].[intervalMs] FROM INT NOT NULL;
GO

CREATE TYPE [core].[number] FROM INT NOT NULL;
GO

-- nullable variants
CREATE TYPE [core].[labelNullable] FROM NVARCHAR(64) NULL;
GO

CREATE TYPE [core].[actorNullable] FROM NVARCHAR(128) NULL;
GO

CREATE TYPE [core].[jsonNullable] FROM NVARCHAR(MAX) NULL;
GO

-- flags
CREATE TYPE [core].[flag] FROM BIT NOT NULL;
GO

-- table-valued references
CREATE TYPE [core].[ActorRef] AS TABLE
(
    [Actor] [core].[actor]
);
GO

CREATE OR ALTER FUNCTION [core].[GetActor]
(
    @Actor [core].[ActorRef] READONLY
)
RETURNS [core].[actor]
AS
BEGIN
    DECLARE @Result [core].[actor];

    SELECT TOP (1) @Result = ar.[Actor]
    FROM @Actor AS ar;

    RETURN @Result;
END
GO

CREATE OR ALTER PROCEDURE [core].[GuardActor]
    @Actor [core].[ActorRef] READONLY,
    @ActorValue [core].[actor] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SET @ActorValue = [core].[GetActor](@Actor);

    IF @ActorValue IS NULL
    BEGIN;
        EXEC [core].[ThrowActorRequired];
    END
END
GO

CREATE OR ALTER PROCEDURE [core].[ThrowActorRequired]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50004, 'Actor reference required', 1;
END
GO
