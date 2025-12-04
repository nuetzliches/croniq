-- Core schema and user-defined types
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'core') EXEC ('CREATE SCHEMA [core]');
GO

-- primitives (NOT NULL)
CREATE TYPE [core].[key] FROM INT NOT NULL;
GO

CREATE TYPE [core].[keyBig] FROM BIGINT NOT NULL;
GO

CREATE TYPE [core].[uid] FROM UNIQUEIDENTIFIER NOT NULL;
GO

CREATE TYPE [core].[reference] FROM NVARCHAR(64) NOT NULL;
GO

CREATE TYPE [core].[tag] FROM NVARCHAR(32) NOT NULL;
GO

CREATE TYPE [core].[label] FROM NVARCHAR(64) NOT NULL;
GO

CREATE TYPE [core].[name] FROM NVARCHAR(128) NOT NULL;
GO

CREATE TYPE [core].[principal] FROM NVARCHAR(128) NOT NULL;
GO

CREATE TYPE [core].[utcDateTime] FROM DATETIME2(3) NOT NULL;
GO

CREATE TYPE [core].[utcDateTimeNullable] FROM DATETIME2(3) NULL;
GO

CREATE TYPE [core].[countInt] FROM INT NOT NULL;
GO

CREATE TYPE [core].[intervalMs] FROM INT NOT NULL;
GO

-- nullable variants
CREATE TYPE [core].[labelNullable] FROM NVARCHAR(64) NULL;
GO

CREATE TYPE [core].[principalNullable] FROM NVARCHAR(128) NULL;
GO

CREATE TYPE [core].[jsonNullable] FROM NVARCHAR(MAX) NULL;
GO

-- flags
CREATE TYPE [core].[flag] FROM BIT NOT NULL;
GO
