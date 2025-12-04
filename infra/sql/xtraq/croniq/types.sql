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
