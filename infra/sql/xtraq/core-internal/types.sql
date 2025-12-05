-- Core-internal schema for guard/throw routines
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'core-internal') 
    EXEC ('CREATE SCHEMA [core-internal]');
GO
