-- Dev seed for Croniq/Xtraq DB
-- Idempotent: checks for existing tenant/client/key before inserting/issuing.

DECLARE @TenantRef NVARCHAR(64) = N'dev-seed';
DECLARE @TenantName NVARCHAR(64) = N'Dev Seed Tenant';
DECLARE @TenantId INT;

IF EXISTS (SELECT 1 FROM [auth].[Tenants] WHERE [Reference] = @TenantRef AND [IsDeleted] = 0)
BEGIN
    SELECT @TenantId = [TenantId] FROM [auth].[Tenants] WHERE [Reference] = @TenantRef AND [IsDeleted] = 0;
END
ELSE
BEGIN
    INSERT INTO [auth].[Tenants] ([Reference], [Name], [CreatedUtc], [CreatedBy])
    VALUES (@TenantRef, @TenantName, SYSUTCDATETIME(), N'seed');
    SET @TenantId = SCOPE_IDENTITY();
END

DECLARE @ClientName NVARCHAR(64) = N'default';
DECLARE @ClientId INT;

IF EXISTS (SELECT 1 FROM [auth].[ApiClients] WHERE [TenantId] = @TenantId AND [Name] = @ClientName AND [IsDeleted] = 0)
BEGIN
    SELECT @ClientId = [ClientId] FROM [auth].[ApiClients] WHERE [TenantId] = @TenantId AND [Name] = @ClientName AND [IsDeleted] = 0;
END
ELSE
BEGIN
    INSERT INTO [auth].[ApiClients]
    (
        [TenantId],
        [Name],
        [Environment],
        [Scopes],
        [CreatedUtc],
        [CreatedBy]
    )
    VALUES
    (
        @TenantId,
        @ClientName,
        N'dev',
        N'["schedules:write","jobs:trigger"]',
        SYSUTCDATETIME(),
        N'seed'
    );
    SET @ClientId = SCOPE_IDENTITY();
END

-- Issue API key only if none is active
IF NOT EXISTS (
    SELECT 1
    FROM [auth].[ApiKeys]
    WHERE [TenantId] = @TenantId
      AND [ClientId] = @ClientId
      AND [RevokedUtc] IS NULL
      AND ([ExpiresUtc] IS NULL OR [ExpiresUtc] > SYSUTCDATETIME())
)
BEGIN
    DECLARE @KeyId INT, @KeyRef NVARCHAR(64), @PlaintextKey NVARCHAR(512), @SecretPreview NVARCHAR(16), @Expires DATETIME2(3);
    EXEC [auth].[ApiKeyIssue]
        @TenantId,
        @ClientId,
        N'dev',
        N'["schedules:write","jobs:trigger"]',
        NULL,
        N'seed',
        @KeyId OUTPUT,
        @KeyRef OUTPUT,
        @PlaintextKey OUTPUT,
        @SecretPreview OUTPUT,
        @Expires OUTPUT;
    PRINT CONCAT('Seed API key issued for tenant ', @TenantRef, ' (client ', @ClientName, '): ', @PlaintextKey);
END
ELSE
BEGIN
    PRINT CONCAT('Seed data already present for tenant ', @TenantRef, ' / client ', @ClientName, ', skipping key issue.');
END
