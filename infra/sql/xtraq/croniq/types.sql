-- Croniq schema domain types
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'croniq')
    EXEC ('CREATE SCHEMA [croniq]');
GO

IF TYPE_ID(N'croniq.jobVariant') IS NULL
    CREATE TYPE [croniq].[jobVariant] FROM NVARCHAR(64) NULL;
GO

IF TYPE_ID(N'croniq.deadLetterReason') IS NULL
    CREATE TYPE [croniq].[deadLetterReason] FROM NVARCHAR(128) NULL;
GO

IF TYPE_ID(N'croniq.cronExpression') IS NULL
    CREATE TYPE [croniq].[cronExpression] FROM NVARCHAR(256) NOT NULL;
GO

IF TYPE_ID(N'croniq.timeZoneId') IS NULL
    CREATE TYPE [croniq].[timeZoneId] FROM NVARCHAR(64) NOT NULL;
GO

IF TYPE_ID(N'croniq.stateCode') IS NULL
    CREATE TYPE [croniq].[stateCode] FROM NVARCHAR(32) NOT NULL;
GO

IF TYPE_ID(N'croniq.InstanceRef') IS NULL
    CREATE TYPE [croniq].[InstanceRef] AS TABLE
    (
        [InstanceId] [core].[reference],
        [Environment] [core].[tag],
        [NodeName] [core].[label],
        [Capabilities] [core].[jsonNullable],
        [Version] [core].[label]
    );
GO

IF TYPE_ID(N'croniq.JobRef') IS NULL
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

IF TYPE_ID(N'croniq.TriggerRef') IS NULL
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
        [NextFireAtUtc] [core].[utcDateTimeNullable],
        [Enabled] [core].[flag],
        [Metadata] [core].[jsonNullable]
    );
GO

IF TYPE_ID(N'croniq.TriggerLeaseRef') IS NULL
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

IF TYPE_ID(N'croniq.TriggerLeaseReleaseRef') IS NULL
    CREATE TYPE [croniq].[TriggerLeaseReleaseRef] AS TABLE
    (
        [LeaseId] [core].[keyBig],
        [InstanceId] [core].[reference],
        [Succeeded] [core].[flag],
        [NextFireAtUtc] [core].[utcDateTimeNullable],
        [DeadLetterReason] [croniq].[deadLetterReason]
    );
GO

IF TYPE_ID(N'croniq.TriggerDeadLetterRef') IS NULL
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
