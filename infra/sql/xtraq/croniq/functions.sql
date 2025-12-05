SET QUOTED_IDENTIFIER ON;
SET ANSI_NULLS ON;
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceId]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[reference]
AS
BEGIN
    DECLARE @Result [core].[reference];

    SELECT TOP (1) @Result = ir.[InstanceId]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceEnvironment]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[tag]
AS
BEGIN
    DECLARE @Result [core].[tag];

    SELECT TOP (1) @Result = ir.[Environment]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceNodeName]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[label]
AS
BEGIN
    DECLARE @Result [core].[label];

    SELECT TOP (1) @Result = ir.[NodeName]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceCapabilities]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[jsonNullable]
AS
BEGIN
    DECLARE @Result [core].[jsonNullable];

    SELECT TOP (1) @Result = ir.[Capabilities]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO

CREATE OR ALTER FUNCTION [croniq].[GetInstanceVersion]
(
    @Instance [croniq].[InstanceRef] READONLY
)
RETURNS [core].[labelNullable]
AS
BEGIN
    DECLARE @Result [core].[label];

    SELECT TOP (1) @Result = ir.[Version]
    FROM @Instance AS ir;

    RETURN @Result;
END
GO
