-- Croniq-internal schema for guard/throw helpers
IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = 'croniq-internal') 
    EXEC ('CREATE SCHEMA [croniq-internal]');
GO
