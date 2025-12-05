SET ANSI_NULLS ON;
SET QUOTED_IDENTIFIER ON;
GO

CREATE OR ALTER PROCEDURE [auth].[ApiKeyIssue]
    @TenantId [core].[key],
    @ClientId [core].[key],
    @Environment [core].[tagNullable],
    @Scopes [auth].[scopes],
    @TtlMinutes [core].[numberNullable],
    @CreatedBy [core].[actor],
    @KeyId [core].[key] OUTPUT,
    @KeyRef [auth].[apiKeyRef] OUTPUT,
    @PlaintextKey [auth].[presentedKey] OUTPUT,
    @SecretPreview [auth].[secretPreview] OUTPUT,
    @ExpiresUtc [core].[utcDateTimeNullable] OUTPUT
AS
BEGIN
    DECLARE @Now [core].[utcDateTime] = SYSUTCDATETIME();
    SET @ExpiresUtc = CASE WHEN @TtlMinutes IS NULL THEN NULL ELSE DATEADD(MINUTE, @TtlMinutes, @Now) END;
    SET @KeyRef = CONCAT('ak_', RIGHT(CONVERT(NVARCHAR(36), NEWID()), 18));
    DECLARE @Secret [auth].[secret] = REPLACE(CONVERT(NVARCHAR(128), NEWID()), '-', '') + REPLACE(CONVERT(NVARCHAR(128), NEWID()), '-', '');
    DECLARE @SecretHash [auth].[apiKeyHash] = HASHBYTES('SHA2_256', CONVERT(VARBINARY(512), @Secret));
    SET @SecretPreview = SUBSTRING(@Secret, 1, 16);
    SET @PlaintextKey = CONCAT(@KeyRef, '.', @Secret);

    INSERT INTO [auth].[ApiKeys]
    (
        [KeyRef],
        [ClientId],
        [TenantId],
        [Environment],
        [Scopes],
        [Hash],
        [Salt],
        [SecretPreview],
        [ExpiresUtc],
        [CreatedUtc],
        [CreatedBy]
    )
    VALUES
    (
        @KeyRef,
        @ClientId,
        @TenantId,
        @Environment,
        @Scopes,
        @SecretHash,
        DEFAULT,
        @SecretPreview,
        @ExpiresUtc,
        @Now,
        @CreatedBy
    );

    SET @KeyId = CAST(SCOPE_IDENTITY() AS INT);
END
GO

CREATE OR ALTER PROCEDURE [auth].[ApiKeyRevoke]
    @TenantId [core].[key],
    @KeyRef [auth].[apiKeyRef],
    @Actor [core].[actor],
    @Reason [core].[labelNullable],
    @Affected [core].[number] OUTPUT
AS
BEGIN
    UPDATE [auth].[ApiKeys]
    SET [RevokedUtc] = SYSUTCDATETIME(),
        [RevokedBy] = @Actor,
        [RevocationReason] = @Reason
    WHERE [TenantId] = @TenantId
      AND [KeyRef] = @KeyRef
      AND [RevokedUtc] IS NULL;

    SET @Affected = @@ROWCOUNT;
END
GO

CREATE OR ALTER PROCEDURE [auth].[ApiKeyRotate]
    @TenantId [core].[key],
    @KeyRef [auth].[apiKeyRef],
    @Actor [core].[actor],
    @PlaintextKey [auth].[presentedKey] OUTPUT,
    @SecretPreview [auth].[secretPreview] OUTPUT
AS
BEGIN
    DECLARE @ClientId [core].[key];
    DECLARE @Environment [core].[tagNullable];
    DECLARE @Scopes [auth].[scopes];

    SELECT TOP (1)
        @ClientId = [ClientId],
        @Environment = [Environment],
        @Scopes = [Scopes]
    FROM [auth].[ApiKeys]
    WHERE [TenantId] = @TenantId
      AND [KeyRef] = @KeyRef
      AND [RevokedUtc] IS NULL
    ORDER BY [KeyId] DESC;

    IF @ClientId IS NULL
    BEGIN
        SET @PlaintextKey = NULL;
        SET @SecretPreview = NULL;
        RETURN;
    END

    DECLARE @Affected [core].[number];
    EXEC [auth].[ApiKeyRevoke] @TenantId, @KeyRef, @Actor, 'rotation', @Affected OUTPUT;

    DECLARE @KeyId [core].[key];
    DECLARE @NewKeyRef [auth].[apiKeyRef];
    DECLARE @Expires [core].[utcDateTimeNullable];
    EXEC [auth].[ApiKeyIssue]
        @TenantId,
        @ClientId,
        @Environment,
        @Scopes,
        NULL,
        @Actor,
        @KeyId OUTPUT,
        @NewKeyRef OUTPUT,
        @PlaintextKey OUTPUT,
        @SecretPreview OUTPUT,
        @Expires OUTPUT;

    -- outputs already populated
END
GO

CREATE OR ALTER PROCEDURE [auth].[ApiKeyValidate]
    @Presented [auth].[presentedKey]
AS
BEGIN
    DECLARE @KeyRef [auth].[apiKeyRef] = NULL;
    DECLARE @Secret [auth].[presentedKey] = @Presented;
    DECLARE @dot [core].[number] = CHARINDEX('.', @Presented);
    IF (@dot > 1)
    BEGIN
        SET @KeyRef = SUBSTRING(@Presented, 1, @dot - 1);
        SET @Secret = SUBSTRING(@Presented, @dot + 1, 512);
    END

    DECLARE @SecretHash [auth].[apiKeyHash] = HASHBYTES('SHA2_256', CONVERT(VARBINARY(512), @Secret));
    DECLARE @Now [core].[utcDateTime] = SYSUTCDATETIME();

    ;WITH candidate AS
    (
        SELECT TOP (1)
            k.[KeyRef],
            k.[TenantId],
            k.[Environment],
            k.[Scopes],
            k.[ExpiresUtc],
            k.[RevokedUtc],
            k.[Hash],
            c.[ClientId],
            c.[Environment] AS ClientEnvironment,
            c.[Scopes] AS ClientScopes
        FROM [auth].[ApiKeys] AS k
        JOIN [auth].[ApiClients] AS c ON c.[ClientId] = k.[ClientId]
        WHERE k.[RevokedUtc] IS NULL
          AND c.[IsDeleted] = 0
          AND (@KeyRef IS NOT NULL OR k.[Hash] = @SecretHash)
          AND (@KeyRef IS NULL OR k.[KeyRef] = @KeyRef)
        ORDER BY k.[KeyId] DESC
    )
    SELECT TOP (1)
        CASE
            WHEN cand.[KeyRef] IS NULL THEN CAST(0 AS BIT)
            WHEN cand.[ExpiresUtc] IS NOT NULL AND cand.[ExpiresUtc] < @Now THEN CAST(0 AS BIT)
            WHEN cand.[Hash] <> @SecretHash THEN CAST(0 AS BIT)
            WHEN cand.[RevokedUtc] IS NOT NULL THEN CAST(0 AS BIT)
            ELSE CAST(1 AS BIT)
        END AS IsValid,
        cand.[TenantId],
        COALESCE(cand.[Environment], cand.[ClientEnvironment]) AS Environment,
        cand.[KeyRef] AS CallerId,
        COALESCE(cand.[Scopes], cand.[ClientScopes]) AS Scopes,
        CASE
            WHEN cand.[KeyRef] IS NULL THEN 'not-found'
            WHEN cand.[ExpiresUtc] IS NOT NULL AND cand.[ExpiresUtc] < @Now THEN 'expired'
            WHEN cand.[RevokedUtc] IS NOT NULL THEN 'revoked'
            WHEN cand.[Hash] <> @SecretHash THEN 'invalid-secret'
            ELSE NULL
        END AS Failure
    FROM candidate AS cand;
END
GO

CREATE OR ALTER PROCEDURE [auth].[ApiClientGet]
    @TenantId [core].[key],
    @ClientId [core].[key]
AS
BEGIN
    SELECT TOP (1)
        [ClientId],
        [TenantId],
        [Name],
        [Environment],
        [Scopes],
        [IsDeleted]
    FROM [auth].[ApiClients]
    WHERE [TenantId] = @TenantId
      AND [ClientId] = @ClientId
      AND [IsDeleted] = 0;
END
GO
