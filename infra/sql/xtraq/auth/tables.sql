SET ANSI_NULLS ON;
SET QUOTED_IDENTIFIER ON;
GO

IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = 'Tenants' AND SCHEMA_NAME(schema_id) = 'auth')
CREATE TABLE [auth].[Tenants]
(
    [TenantId] [core].[key] IDENTITY(1001,1) PRIMARY KEY,
    [Reference] [core].[reference],
    [Name] [core].[label],
    [CreatedUtc] [core].[utcDateTime] CONSTRAINT DF_auth_Tenants_CreatedUtc DEFAULT SYSUTCDATETIME(),
    [CreatedBy] [core].[actor],
    [UpdatedUtc] [core].[utcDateTimeNullable],
    [UpdatedBy] [core].[actorNullable],
    [IsDeleted] [core].[flag] CONSTRAINT DF_auth_Tenants_IsDeleted DEFAULT (0)
);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'UX_auth_Tenants_Reference' AND object_id = OBJECT_ID('[auth].[Tenants]'))
CREATE UNIQUE NONCLUSTERED INDEX [UX_auth_Tenants_Reference]
    ON [auth].[Tenants] ([Reference])
    WHERE [IsDeleted] = 0;
GO

IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = 'Users' AND SCHEMA_NAME(schema_id) = 'auth')
CREATE TABLE [auth].[Users]
(
    [UserId] [core].[key] IDENTITY(2001,1) PRIMARY KEY,
    [TenantId] [core].[key],
    [Subject] [auth].[subject],
    [Issuer] [auth].[issuer],
    [Email] [core].[labelNullable],
    [DisplayName] [core].[labelNullable],
    [CreatedUtc] [core].[utcDateTime] CONSTRAINT DF_auth_Users_CreatedUtc DEFAULT SYSUTCDATETIME(),
    [CreatedBy] [core].[actor],
    [UpdatedUtc] [core].[utcDateTimeNullable],
    [UpdatedBy] [core].[actorNullable],
    [IsDeleted] [core].[flag] CONSTRAINT DF_auth_Users_IsDeleted DEFAULT (0),
    CONSTRAINT FK_auth_Users_Tenant FOREIGN KEY ([TenantId]) REFERENCES [auth].[Tenants]([TenantId])
);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'UX_auth_Users_Subject' AND object_id = OBJECT_ID('[auth].[Users]'))
CREATE UNIQUE NONCLUSTERED INDEX [UX_auth_Users_Subject]
    ON [auth].[Users] ([TenantId], [Issuer], [Subject])
    WHERE [IsDeleted] = 0;
GO

IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = 'UserRoles' AND SCHEMA_NAME(schema_id) = 'auth')
CREATE TABLE [auth].[UserRoles]
(
    [UserId] [core].[key],
    [Role] [core].[label],
    CONSTRAINT PK_auth_UserRoles PRIMARY KEY ([UserId], [Role]),
    CONSTRAINT FK_auth_UserRoles_User FOREIGN KEY ([UserId]) REFERENCES [auth].[Users]([UserId])
);
GO

IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = 'ApiClients' AND SCHEMA_NAME(schema_id) = 'auth')
CREATE TABLE [auth].[ApiClients]
(
    [ClientId] [core].[key] IDENTITY(3001,1) PRIMARY KEY,
    [TenantId] [core].[key],
    [Name] [core].[labelNullable],
    [Environment] [core].[tagNullable],
    [Scopes] [core].[jsonNullable],
    [CreatedUtc] [core].[utcDateTime] CONSTRAINT DF_auth_ApiClients_CreatedUtc DEFAULT SYSUTCDATETIME(),
    [CreatedBy] [core].[actor],
    [UpdatedUtc] [core].[utcDateTimeNullable],
    [UpdatedBy] [core].[actorNullable],
    [IsDeleted] [core].[flag] CONSTRAINT DF_auth_ApiClients_IsDeleted DEFAULT (0),
    CONSTRAINT FK_auth_ApiClients_Tenant FOREIGN KEY ([TenantId]) REFERENCES [auth].[Tenants]([TenantId])
);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'UX_auth_ApiClients_Name' AND object_id = OBJECT_ID('[auth].[ApiClients]'))
CREATE UNIQUE NONCLUSTERED INDEX [UX_auth_ApiClients_Name]
    ON [auth].[ApiClients] ([TenantId], [Name])
    WHERE [IsDeleted] = 0;
GO

IF NOT EXISTS (SELECT 1 FROM sys.tables WHERE name = 'ApiKeys' AND SCHEMA_NAME(schema_id) = 'auth')
CREATE TABLE [auth].[ApiKeys]
(
    [KeyId] [core].[key] IDENTITY(4001,1) PRIMARY KEY,
    [KeyRef] [auth].[apiKeyRef],
    [ClientId] [core].[key],
    [TenantId] [core].[key],
    [Environment] [core].[tagNullable],
    [Scopes] [core].[jsonNullable],
    [Hash] [auth].[apiKeyHash],
    [Salt] [auth].[apiKeySalt] CONSTRAINT DF_auth_ApiKeys_Salt DEFAULT (0x),
    [SecretPreview] [auth].[secretPreview],
    [ExpiresUtc] [core].[utcDateTimeNullable],
    [CreatedUtc] [core].[utcDateTime] CONSTRAINT DF_auth_ApiKeys_CreatedUtc DEFAULT SYSUTCDATETIME(),
    [CreatedBy] [core].[actor],
    [RevokedUtc] [core].[utcDateTimeNullable],
    [RevokedBy] [core].[actorNullable],
    [RevocationReason] [core].[labelNullable],
    CONSTRAINT FK_auth_ApiKeys_Client FOREIGN KEY ([ClientId]) REFERENCES [auth].[ApiClients]([ClientId]),
    CONSTRAINT FK_auth_ApiKeys_Tenant FOREIGN KEY ([TenantId]) REFERENCES [auth].[Tenants]([TenantId])
);
GO

IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE name = 'UX_auth_ApiKeys_KeyRef' AND object_id = OBJECT_ID('[auth].[ApiKeys]'))
CREATE UNIQUE NONCLUSTERED INDEX [UX_auth_ApiKeys_KeyRef]
    ON [auth].[ApiKeys] ([KeyRef])
    WHERE [RevokedUtc] IS NULL;
GO
