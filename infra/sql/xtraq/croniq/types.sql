-- Croniq schema domain types
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'croniq') EXEC ('CREATE SCHEMA [croniq]');
GO

CREATE TYPE [croniq].[jobVariant] FROM NVARCHAR(64) NULL;
GO

CREATE TYPE [croniq].[deadLetterReason] FROM NVARCHAR(128) NULL;
GO

CREATE TYPE [croniq].[cronExpression] FROM NVARCHAR(256) NOT NULL;
GO

CREATE TYPE [croniq].[timeZoneId] FROM NVARCHAR(64) NOT NULL;
GO

CREATE TYPE [croniq].[stateCode] FROM NVARCHAR(32) NOT NULL;
GO

CREATE TYPE [croniq].[InstanceRef] AS TABLE
(
    [InstanceId] [core].[reference],
    [Environment] [core].[tag],
    [NodeName] [core].[label],
    [Capabilities] [core].[jsonNullable],
    [Version] [core].[labelNullable]
);
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceId]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[reference]
AS
BEGIN
    DECLARE @Result [core].[reference];

    SELECT TOP (1) @Result = ir.[InstanceId]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[GuardInstanceRef]
    @Instance [croniq].[InstanceRef] READONLY,
    @InstanceId [core].[reference] OUTPUT,
    @Environment [core].[tag] OUTPUT,
    @NodeName [core].[label] OUTPUT,
    @Capabilities [core].[jsonNullable] OUTPUT,
    @Version [core].[labelNullable] OUTPUT
AS
BEGIN
    SET NOCOUNT ON;

    SELECT TOP (1)
        @InstanceId = ir.[InstanceId],
        @Environment = ir.[Environment],
        @NodeName = ir.[NodeName],
        @Capabilities = ir.[Capabilities],
        @Version = ir.[Version]
    FROM @Instance AS ir;

    IF @InstanceId IS NULL OR @Environment IS NULL OR @NodeName IS NULL
    BEGIN;
        EXEC [croniq].[ThrowInstanceRefIncomplete];
    END
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowInstanceRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50002, 'Instance reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowInstanceNotRegistered]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50001, 'Instance not registered', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowInstanceIdentityMismatch]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50003, 'Instance identity mismatch', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq].[ThrowInstanceReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50005, 'Instance reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceEnvironment]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[tag]
AS
BEGIN
    DECLARE @Result [core].[tag];

    SELECT TOP (1) @Result = ir.[Environment]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceNodeName]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[label]
AS
BEGIN
    DECLARE @Result [core].[label];

    SELECT TOP (1) @Result = ir.[NodeName]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceCapabilities]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[jsonNullable]
AS
BEGIN
    DECLARE @Result [core].[jsonNullable];

    SELECT TOP (1) @Result = ir.[Capabilities]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceVersion]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[labelNullable]
AS
BEGIN
    DECLARE @Result [core].[labelNullable];

    SELECT TOP (1) @Result = ir.[Version]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO
