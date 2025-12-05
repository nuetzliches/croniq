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
    [Version] [core].[label]
);
GO

CREATE TYPE [croniq].[JobRef] AS TABLE
(
    [JobKey] [core].[reference],
    [TenantId] [core].[key],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [Description] [core].[labelNullable],
    [Metadata] [core].[jsonNullable]
);
GO

CREATE TYPE [croniq].[TriggerRef] AS TABLE
(
    [TriggerKey] [core].[reference],
    [JobKey] [core].[reference],
    [TenantId] [core].[key],
    [JobId] [core].[keyBig],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [CronExpression] [croniq].[cronExpression],
    [TimeZoneId] [croniq].[timeZoneId],
    [StartAtUtc] [core].[utcDateTimeNullable],
    [EndAtUtc] [core].[utcDateTimeNullable],
    [Enabled] [core].[flag],
    [Metadata] [core].[jsonNullable]
);
GO

CREATE TYPE [croniq].[TriggerLeaseRef] AS TABLE
(
    [TriggerId] [core].[keyBig],
    [JobId] [core].[keyBig],
    [TenantId] [core].[key],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [InstanceId] [core].[reference],
    [FireAtUtc] [core].[utcDateTime],
    [LeaseExpiresAtUtc] [core].[utcDateTime],
    [Payload] [core].[jsonNullable]
);
GO

CREATE TYPE [croniq].[TriggerLeaseReleaseRef] AS TABLE
(
    [LeaseId] [core].[keyBig],
    [InstanceId] [core].[reference]
);
GO

CREATE TYPE [croniq].[TriggerDeadLetterRef] AS TABLE
(
    [TriggerId] [core].[keyBig],
    [TenantId] [core].[key],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [FireAtUtc] [core].[utcDateTime],
    [DeadLetterReason] [croniq].[deadLetterReason],
    [Payload] [core].[jsonNullable]
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
    DECLARE @Result [core].[label];

    SELECT TOP (1) @Result = ir.[Version]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO
