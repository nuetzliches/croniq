-- Auth schema tables
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'auth') 
    EXEC ('CREATE SCHEMA [auth]');
GO

-- auth-specific types
IF NOT EXISTS (SELECT 1 FROM sys.types WHERE is_table_type = 0 AND name = 'subject' AND SCHEMA_NAME(schema_id) = 'auth')
    CREATE TYPE [auth].[subject] FROM NVARCHAR(256) NOT NULL;
GO

IF NOT EXISTS (SELECT 1 FROM sys.types WHERE is_table_type = 0 AND name = 'issuer' AND SCHEMA_NAME(schema_id) = 'auth')
    CREATE TYPE [auth].[issuer] FROM NVARCHAR(256) NOT NULL;
GO

IF NOT EXISTS (SELECT 1 FROM sys.types WHERE is_table_type = 0 AND name = 'apiKeyHash' AND SCHEMA_NAME(schema_id) = 'auth')
    CREATE TYPE [auth].[apiKeyHash] FROM VARBINARY(64) NOT NULL;
GO

IF NOT EXISTS (SELECT 1 FROM sys.types WHERE is_table_type = 0 AND name = 'apiKeySalt' AND SCHEMA_NAME(schema_id) = 'auth')
    CREATE TYPE [auth].[apiKeySalt] FROM VARBINARY(32) NOT NULL;
GO

IF NOT EXISTS (SELECT 1 FROM sys.types WHERE is_table_type = 0 AND name = 'apiKeyRef' AND SCHEMA_NAME(schema_id) = 'auth')
    CREATE TYPE [auth].[apiKeyRef] FROM NVARCHAR(64) NOT NULL;
GO

IF NOT EXISTS (SELECT 1 FROM sys.types WHERE is_table_type = 0 AND name = 'scopes' AND SCHEMA_NAME(schema_id) = 'auth')
    CREATE TYPE [auth].[scopes] FROM NVARCHAR(MAX) NULL;
GO

IF NOT EXISTS (SELECT 1 FROM sys.types WHERE is_table_type = 0 AND name = 'presentedKey' AND SCHEMA_NAME(schema_id) = 'auth')
    CREATE TYPE [auth].[presentedKey] FROM NVARCHAR(512) NOT NULL;
GO

IF NOT EXISTS (SELECT 1 FROM sys.types WHERE is_table_type = 0 AND name = 'secretPreview' AND SCHEMA_NAME(schema_id) = 'auth')
    CREATE TYPE [auth].[secretPreview] FROM NVARCHAR(16) NULL;
GO

IF NOT EXISTS (SELECT 1 FROM sys.types WHERE is_table_type = 0 AND name = 'secret' AND SCHEMA_NAME(schema_id) = 'auth')
    CREATE TYPE [auth].[secret] FROM NVARCHAR(128) NOT NULL;
GO
