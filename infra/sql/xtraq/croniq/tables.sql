-- Croniq tables
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'croniq') EXEC ('CREATE SCHEMA [croniq]');
GO

CREATE TABLE [croniq].[Instances]
(
    [InstanceId] [core].[reference] PRIMARY KEY,
    [Environment] [core].[tag],
    [NodeName] [core].[label],
    [Capabilities] [core].[jsonNullable],
    [Version] [core].[labelNullable],
    [Generation] [core].[count] CONSTRAINT DF_croniq_Instances_Generation DEFAULT (1),
    [StartedUtc] [core].[utcDateTime] CONSTRAINT DF_croniq_Instances_StartedUtc DEFAULT SYSUTCDATETIME(),
    [LastSeenUtc] [core].[utcDateTime] CONSTRAINT DF_croniq_Instances_LastSeenUtc DEFAULT SYSUTCDATETIME(),
    [CreatedUtc] [core].[utcDateTime] CONSTRAINT DF_croniq_Instances_CreatedUtc DEFAULT SYSUTCDATETIME(),
    [CreatedBy] [core].[actor],
    [UpdatedUtc] [core].[utcDateTimeNullable],
    [UpdatedBy] [core].[actorNullable],
    [IsDeleted] [core].[flag] CONSTRAINT DF_croniq_Instances_IsDeleted DEFAULT (0)
);
GO

CREATE TABLE [croniq].[Jobs]
(
    [JobId] [core].[keyBig] IDENTITY(1001,1) PRIMARY KEY,
    [TenantId] [core].[key],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [Description] [core].[labelNullable],
    [Metadata] [core].[jsonNullable],
    [CreatedUtc] [core].[utcDateTime] CONSTRAINT DF_croniq_Jobs_CreatedUtc DEFAULT SYSUTCDATETIME(),
    [CreatedBy] [core].[actor],
    [UpdatedUtc] [core].[utcDateTimeNullable],
    [UpdatedBy] [core].[actorNullable],
    [IsDeleted] [core].[flag] CONSTRAINT DF_croniq_Jobs_IsDeleted DEFAULT (0),
    CONSTRAINT FK_croniq_Jobs_auth_Tenants FOREIGN KEY ([TenantId]) REFERENCES [auth].[Tenants]([TenantId])
);
GO

CREATE UNIQUE NONCLUSTERED INDEX [UX_croniq_Jobs_Key]
ON [croniq].[Jobs] ([TenantId], [Environment], [Namespace], [Name], [Variant])
WHERE [IsDeleted] = 0;
GO

CREATE NONCLUSTERED INDEX [IX_croniq_TriggerLeases_StaleCheck]
ON [croniq].[TriggerLeases] ([IsDeleted], [LeaseExpiresAtUtc])
INCLUDE ([InstanceId]);
GO

CREATE NONCLUSTERED INDEX [IX_croniq_Instances_LastSeen]
ON [croniq].[Instances] ([IsDeleted], [InstanceId])
INCLUDE ([LastSeenUtc]);
GO

CREATE TABLE [croniq].[Triggers]
(
    [TriggerId] [core].[uid] CONSTRAINT DF_croniq_Triggers_Id DEFAULT NEWSEQUENTIALID() PRIMARY KEY,
    [JobId] [core].[keyBig],
    [TenantId] [core].[key],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [CronExpression] [croniq].[cronExpression],
    [TimeZoneId] [croniq].[timeZoneId],
    [StartAtUtc] [core].[utcDateTimeNullable],
    [EndAtUtc] [core].[utcDateTimeNullable],
    [Enabled] [core].[flag] CONSTRAINT DF_croniq_Triggers_Enabled DEFAULT (1),
    [Metadata] [core].[jsonNullable],
    [CreatedUtc] [core].[utcDateTime] CONSTRAINT DF_croniq_Triggers_CreatedUtc DEFAULT SYSUTCDATETIME(),
    [CreatedBy] [core].[actor],
    [UpdatedUtc] [core].[utcDateTimeNullable],
    [UpdatedBy] [core].[actorNullable],
    [IsDeleted] [core].[flag] CONSTRAINT DF_croniq_Triggers_IsDeleted DEFAULT (0),
    CONSTRAINT FK_croniq_Triggers_croniq_Jobs FOREIGN KEY ([JobId]) REFERENCES [croniq].[Jobs]([JobId]),
    CONSTRAINT FK_croniq_Triggers_auth_Tenants FOREIGN KEY ([TenantId]) REFERENCES [auth].[Tenants]([TenantId])
);
GO

CREATE UNIQUE NONCLUSTERED INDEX [UX_croniq_Triggers_Key]
    ON [croniq].[Triggers] ([TenantId], [Environment], [Namespace], [Name], [Variant])
    WHERE [IsDeleted] = 0;
GO

CREATE TABLE [croniq].[TriggerLeases]
(
    [LeaseId] [core].[uid] CONSTRAINT DF_croniq_TriggerLeases_Id DEFAULT NEWSEQUENTIALID() PRIMARY KEY,
    [TriggerId] [core].[uid],
    [JobId] [core].[keyBig],
    [TenantId] [core].[key],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [InstanceId] [core].[reference],
    [FireAtUtc] [core].[utcDateTime],
    [LeaseExpiresAtUtc] [core].[utcDateTime],
    [Payload] [core].[jsonNullable],
    [CreatedUtc] [core].[utcDateTime] CONSTRAINT DF_croniq_TriggerLeases_CreatedUtc DEFAULT SYSUTCDATETIME(),
    [CreatedBy] [core].[actor],
    [UpdatedUtc] [core].[utcDateTimeNullable],
    [UpdatedBy] [core].[actorNullable],
    [IsDeleted] [core].[flag] CONSTRAINT DF_croniq_TriggerLeases_IsDeleted DEFAULT (0),
    CONSTRAINT FK_croniq_TriggerLeases_croniq_Triggers FOREIGN KEY ([TriggerId]) REFERENCES [croniq].[Triggers]([TriggerId]),
    CONSTRAINT FK_croniq_TriggerLeases_croniq_Jobs FOREIGN KEY ([JobId]) REFERENCES [croniq].[Jobs]([JobId]),
    CONSTRAINT FK_croniq_TriggerLeases_auth_Tenants FOREIGN KEY ([TenantId]) REFERENCES [auth].[Tenants]([TenantId]),
    CONSTRAINT FK_croniq_TriggerLeases_croniq_Instances FOREIGN KEY ([InstanceId]) REFERENCES [croniq].[Instances]([InstanceId])
);
GO

CREATE TABLE [croniq].[TriggerDeadLetter]
(
    [DeadLetterId] [core].[keyBig] IDENTITY(1001,1) PRIMARY KEY,
    [TriggerId] [core].[uid],
    [TenantId] [core].[key],
    [Environment] [core].[tag],
    [Namespace] [core].[label],
    [Name] [core].[name],
    [Variant] [croniq].[jobVariant],
    [FireAtUtc] [core].[utcDateTime],
    [DeadLetterReason] [croniq].[deadLetterReason],
    [Payload] [core].[jsonNullable],
    [CreatedUtc] [core].[utcDateTime] CONSTRAINT DF_croniq_TriggerDeadLetter_CreatedUtc DEFAULT SYSUTCDATETIME(),
    [CreatedBy] [core].[actor],
    [UpdatedUtc] [core].[utcDateTimeNullable],
    [UpdatedBy] [core].[actorNullable],
    [IsDeleted] [core].[flag] CONSTRAINT DF_croniq_TriggerDeadLetter_IsDeleted DEFAULT (0),
    CONSTRAINT FK_croniq_TriggerDeadLetter_auth_Tenants FOREIGN KEY ([TenantId]) REFERENCES [auth].[Tenants]([TenantId]),
    CONSTRAINT FK_croniq_TriggerDeadLetter_croniq_Triggers FOREIGN KEY ([TriggerId]) REFERENCES [croniq].[Triggers]([TriggerId])
);
GO
