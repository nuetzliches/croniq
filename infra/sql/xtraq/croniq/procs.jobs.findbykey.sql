-- Find job by JobKey and return a JSON payload (single object, no array)
GO

CREATE OR ALTER PROCEDURE [croniq].[JobFindByKey]
    @JobKey [core].[reference]
AS
BEGIN
    SET NOCOUNT ON;

    SELECT [JobId],
        [JobKey],
        [TenantId],
        [Environment],
        [Namespace],
        [Name],
        [Variant],
        [Description],
        [Metadata],
        [CreatedUtc],
        [UpdatedUtc],
        [IsDeleted]
    FROM [croniq].[Jobs]
    WHERE [JobKey] = @JobKey
    FOR JSON PATH, WITHOUT_ARRAY_WRAPPER;
END
GO
