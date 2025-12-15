using System;
using System.Linq;
using System.Threading;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapExecutionEndpoints(WebApplication app)
    {
        app.MapGet("/tenants/{tenantId}/executions", async (
            string tenantId,
            string environment,
            string? jobKey,
            ExecutionStatus? status,
            DateTimeOffset? startedAfterUtc,
            DateTimeOffset? startedBeforeUtc,
            int? limit,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IExecutionHistoryReader historyReader,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            if (startedAfterUtc.HasValue && startedBeforeUtc.HasValue && startedAfterUtc.Value >= startedBeforeUtc.Value)
            {
                return Results.BadRequest(new { error = "invalid-range", message = "startedAfterUtc must be earlier than startedBeforeUtc." });
            }

            if (!string.IsNullOrWhiteSpace(jobKey))
            {
                if (!JobKey.TryParse(jobKey, out var parsed))
                {
                    return Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." });
                }

                if (!string.Equals(parsed.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                    || !string.Equals(parsed.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
                {
                    return Results.Problem(statusCode: StatusCodes.Status403Forbidden, title: "scope-mismatch", detail: "JobKey tenant/environment must match the request scope.");
                }
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, tenantId, environment, CroniqScopes.ExecutionsRead);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var scope = new PartitionScope(tenantId, environment);
            var query = new ExecutionHistoryQuery
            {
                JobKey = jobKey,
                Status = status,
                StartedAfterUtc = startedAfterUtc,
                StartedBeforeUtc = startedBeforeUtc,
                Limit = Math.Clamp(limit ?? ExecutionHistoryQuery.DefaultLimit, 1, ExecutionHistoryQuery.MaxLimit)
            };

            var summaries = await historyReader.ListExecutionsAsync(scope, query, cancellationToken).ConfigureAwait(false);
            var payload = summaries.Select(ToExecutionResponse).ToArray();
            return Results.Ok(payload);
        })
        .WithDocs("Executions_List", "List executions", "Returns execution summaries for the tenant/environment scope with optional filters.");

        app.MapGet("/tenants/{tenantId}/executions/{executionId}", async (
            string tenantId,
            string executionId,
            string environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IExecutionHistoryReader historyReader,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(environment))
            {
                return Results.BadRequest(new { error = "missing-environment", message = "Query parameter 'environment' is required." });
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, tenantId, environment, CroniqScopes.ExecutionsRead);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var summary = await historyReader.GetExecutionAsync(executionId, cancellationToken).ConfigureAwait(false);
            if (summary is null
                || !string.Equals(summary.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
                || !string.Equals(summary.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
            {
                return Results.NotFound(new { error = "execution-not-found", executionId });
            }

            return Results.Ok(ToExecutionResponse(summary));
        })
        .WithDocs("Executions_Get", "Get execution", "Returns metadata for a single execution in the tenant/environment scope.");
    }
}
