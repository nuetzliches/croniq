-- Auth schema tables
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'auth') EXEC ('CREATE SCHEMA [auth]');
GO

CREATE TABLE [auth].[Tenants]
(
    [TenantId] [core].[key] IDENTITY(1001,1) PRIMARY KEY,
    [Reference] [core].[reference] UNIQUE,
    [Name] [core].[label],
    [CreatedUtc] [core].[utcDateTime] CONSTRAINT DF_auth_Tenants_CreatedUtc DEFAULT SYSUTCDATETIME(),
    [CreatedBy] [core].[principal],
    [UpdatedUtc] [core].[utcDateTimeNullable],
    [UpdatedBy] [core].[principalNullable],
    [IsDeleted] [core].[flag] CONSTRAINT DF_auth_Tenants_IsDeleted DEFAULT (0)
);
GO
