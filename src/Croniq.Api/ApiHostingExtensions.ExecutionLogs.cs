using System;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Execution;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapExecutionLogEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapGet("/tenants/{tenantId}/executions/{executionId}/logs", async (
            string tenantId,
            string executionId,
            [FromServices] IExecutionLogReader reader,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            HttpContext httpContext,
            CancellationToken cancellationToken) =>
        {
            await using var enumerator = reader.ReadLinesAsync(executionId, cancellationToken).GetAsyncEnumerator(cancellationToken);
            if (!await enumerator.MoveNextAsync().ConfigureAwait(false))
            {
                await Results.NotFound(new { error = "execution-logs-not-found", executionId })
                    .ExecuteAsync(httpContext)
                    .ConfigureAwait(false);
                return;
            }

            var firstLine = enumerator.Current;
            if (!TryExtractExecutionScope(firstLine, out var logTenantId, out var environmentTag))
            {
                await Results.Problem(
                        statusCode: StatusCodes.Status500InternalServerError,
                        title: "execution-log-invalid",
                        detail: "Execution log entry missing tenant/environment metadata.")
                    .ExecuteAsync(httpContext)
                    .ConfigureAwait(false);
                return;
            }

            if (!string.Equals(logTenantId, tenantId, StringComparison.OrdinalIgnoreCase))
            {
                await Results.StatusCode(StatusCodes.Status403Forbidden)
                    .ExecuteAsync(httpContext)
                    .ConfigureAwait(false);
                return;
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, logTenantId!, environmentTag, CroniqScopes.ExecutionsRead);
            if (authFailure is not null)
            {
                await authFailure.ExecuteAsync(httpContext).ConfigureAwait(false);
                return;
            }

            var response = httpContext.Response;
            response.ContentType = "application/x-ndjson";
            await response.WriteAsync(firstLine, cancellationToken).ConfigureAwait(false);
            await response.WriteAsync("\n", cancellationToken).ConfigureAwait(false);

            while (await enumerator.MoveNextAsync().ConfigureAwait(false))
            {
                await response.WriteAsync(enumerator.Current, cancellationToken).ConfigureAwait(false);
                await response.WriteAsync("\n", cancellationToken).ConfigureAwait(false);
            }
        })
        .WithDocs("Executions_GetLogs", "Stream execution logs", "Streams NDJSON execution logs for a tenant-scoped execution after authorizing tenant scope.")
        .RequireCroniqCaller();
    }
}
