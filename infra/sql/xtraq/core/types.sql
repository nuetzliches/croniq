-- Core schema and user-defined types
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'core') 
    EXEC ('CREATE SCHEMA [core]');
GO

-- primitives (NOT NULL)
IF TYPE_ID(N'core.key') IS NULL
    CREATE TYPE [core].[key] FROM INT NOT NULL;
GO

IF TYPE_ID(N'core.keyBig') IS NULL
    CREATE TYPE [core].[keyBig] FROM BIGINT NOT NULL;
GO

IF TYPE_ID(N'core.uid') IS NULL
    CREATE TYPE [core].[uid] FROM UNIQUEIDENTIFIER NOT NULL;
GO

IF TYPE_ID(N'core.reference') IS NULL
    CREATE TYPE [core].[reference] FROM NVARCHAR(64) NOT NULL;
GO

IF TYPE_ID(N'core.tag') IS NULL
    CREATE TYPE [core].[tag] FROM NVARCHAR(32) NOT NULL;
GO

IF TYPE_ID(N'core.tagNullable') IS NULL
    CREATE TYPE [core].[tagNullable] FROM NVARCHAR(32) NULL;
GO

IF TYPE_ID(N'core.numberNullable') IS NULL
    CREATE TYPE [core].[numberNullable] FROM INT NULL;
GO

IF TYPE_ID(N'core.label') IS NULL
    CREATE TYPE [core].[label] FROM NVARCHAR(64) NOT NULL;
GO

IF TYPE_ID(N'core.name') IS NULL
    CREATE TYPE [core].[name] FROM NVARCHAR(128) NOT NULL;
GO

IF TYPE_ID(N'core.actor') IS NULL
    CREATE TYPE [core].[actor] FROM NVARCHAR(128) NOT NULL;
GO

IF TYPE_ID(N'core.utcDateTime') IS NULL
    CREATE TYPE [core].[utcDateTime] FROM DATETIME2(3) NOT NULL;
GO

IF TYPE_ID(N'core.utcDateTimeNullable') IS NULL
    CREATE TYPE [core].[utcDateTimeNullable] FROM DATETIME2(3) NULL;
GO

IF TYPE_ID(N'core.count') IS NULL
    CREATE TYPE [core].[count] FROM INT NOT NULL;
GO

IF TYPE_ID(N'core.intervalMs') IS NULL
    CREATE TYPE [core].[intervalMs] FROM INT NOT NULL;
GO

IF TYPE_ID(N'core.number') IS NULL
    CREATE TYPE [core].[number] FROM INT NOT NULL;
GO

-- nullable variants
IF TYPE_ID(N'core.labelNullable') IS NULL
    CREATE TYPE [core].[labelNullable] FROM NVARCHAR(64) NULL;
GO

IF TYPE_ID(N'core.actorNullable') IS NULL
    CREATE TYPE [core].[actorNullable] FROM NVARCHAR(128) NULL;
GO

IF TYPE_ID(N'core.jsonNullable') IS NULL
    CREATE TYPE [core].[jsonNullable] FROM NVARCHAR(MAX) NULL;
GO

-- flags
IF TYPE_ID(N'core.flag') IS NULL
    CREATE TYPE [core].[flag] FROM BIT NOT NULL;
GO

-- table-valued references
IF TYPE_ID(N'core.ActorRef') IS NULL
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
