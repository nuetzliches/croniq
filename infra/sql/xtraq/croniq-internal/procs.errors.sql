SET QUOTED_IDENTIFIER ON;
SET ANSI_NULLS ON;
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowInstanceRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50002, 'Instance reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowInstanceNotRegistered]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50001, 'Instance not registered', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowInstanceIdentityMismatch]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50003, 'Instance identity mismatch', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowInstanceReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50005, 'Instance reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowJobRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50006, 'Job reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowJobNotFound]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50007, 'Job not found', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowJobReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50008, 'Job reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50009, 'Trigger reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerNotFound]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50010, 'Trigger not found', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50011, 'Trigger reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerJobMissing]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50012, 'Referenced job not found or deleted', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50013, 'Trigger lease reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseReleaseRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50014, 'Trigger lease release reference incomplete', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseActive]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50015, 'Trigger lease already active', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseNotFound]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50016, 'Trigger lease not found', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseOwnershipMismatch]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50017, 'Trigger lease ownership mismatch', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerLeaseReuseNotAllowed]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50018, 'Trigger lease reuse of soft-deleted entry not allowed', 1;
END
GO

CREATE OR ALTER PROCEDURE [croniq-internal].[ThrowTriggerDeadLetterRefIncomplete]
AS
BEGIN
    SET NOCOUNT ON;
    THROW 50019, 'Trigger dead letter reference incomplete', 1;
END
GO
