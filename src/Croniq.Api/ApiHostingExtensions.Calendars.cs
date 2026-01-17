using System;
using System.Linq;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Core.Scheduling;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapCalendarEndpoints(WebApplication app)
    {
        app.MapGet("/tenants/{tenantId}/calendars", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] ICalendarStore store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var calendars = await store.ListCalendarsAsync(scope, cancellationToken).ConfigureAwait(false);
            var payload = calendars.Select(ToCalendarResponse).ToArray();
            return Results.Ok(payload);
        })
        .WithDocs("Calendars_List", "List calendars", "Returns all calendar definitions for the tenant/environment scope.")
        .Produces<CalendarResponse[]>(StatusCodes.Status200OK)
        .RequireCroniqTenantScope(CroniqScopes.CalendarsRead);

        app.MapGet("/tenants/{tenantId}/calendars/{calendarId}", async (
            string tenantId,
            string calendarId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] ICalendarStore store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (string.IsNullOrWhiteSpace(calendarId))
            {
                return Results.BadRequest(new { error = "invalid-calendar-id", message = "CalendarId is required." });
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var calendar = await store.FindAsync(calendarId, scope, cancellationToken).ConfigureAwait(false);
            if (calendar is null)
            {
                return Results.NotFound(new { error = "calendar-not-found", calendarId });
            }

            return Results.Ok(ToCalendarResponse(calendar));
        })
        .WithDocs("Calendars_Get", "Get calendar", "Returns the calendar definition for the requested calendar id.")
        .Produces<CalendarResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status404NotFound)
        .RequireCroniqTenantScope(CroniqScopes.CalendarsRead);

        app.MapPost("/tenants/{tenantId}/calendars", async (
            string tenantId,
            string? environment,
            CroniqCalendarSeedDefinition request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] ICalendarStore store,
            CancellationToken cancellationToken) =>
        {
            if (!CalendarDefinitionValidator.TryValidate(request, scope: null, out var validation, out var error))
            {
                return Results.BadRequest(new { error = "invalid-request", message = error });
            }

            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var authFailure = TenantGuard.EnsureTenant(callerContextAccessor, tenantId, resolvedEnvironment, CroniqScopes.CalendarsWrite);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var upsert = new CalendarUpsert(
                validation.CalendarId,
                tenantId,
                resolvedEnvironment,
                validation.Name,
                validation.Description,
                validation.TimeZoneId,
                validation.Mode,
                validation.Rules,
                request.Enabled);

            await store.UpsertAsync(upsert, cancellationToken).ConfigureAwait(false);

            return Results.Created(
                $"/tenants/{tenantId}/calendars/{Uri.EscapeDataString(validation.CalendarId)}",
                new CalendarUpsertResult(validation.CalendarId, validation.Name));
        })
        .WithDocs("Calendars_Upsert", "Create or update a calendar", "Creates or updates a calendar definition for the tenant/environment scope.")
        .Produces<CalendarUpsertResult>(StatusCodes.Status201Created)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScope(CroniqScopes.CalendarsWrite);

        app.MapDelete("/tenants/{tenantId}/calendars/{calendarId}", async (
            string tenantId,
            string calendarId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] ICalendarStore store,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            await store.DeleteAsync(calendarId, scope, cancellationToken).ConfigureAwait(false);
            return Results.NoContent();
        })
        .WithDocs("Calendars_Delete", "Delete a calendar", "Deletes the calendar definition for the tenant/environment scope.")
        .Produces(StatusCodes.Status204NoContent)
        .RequireCroniqTenantScope(CroniqScopes.CalendarsWrite);
    }
}
