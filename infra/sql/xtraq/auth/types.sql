-- Auth schema tables
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'auth') 
    EXEC ('CREATE SCHEMA [auth]');
GO
