-- Auth schema tables
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'auth') EXEC ('CREATE SCHEMA [auth]');
GO

SET ANSI_NULLS ON;
SET QUOTED_IDENTIFIER ON;
GO

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

CREATE UNIQUE NONCLUSTERED INDEX [UX_auth_Tenants_Reference]
    ON [auth].[Tenants] ([Reference])
    WHERE [IsDeleted] = 0;
GO
