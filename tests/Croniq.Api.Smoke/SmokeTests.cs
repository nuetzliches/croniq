using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Threading.Tasks;
using FluentAssertions;
using Xunit;

namespace Croniq.Api.Smoke;

public sealed class SmokeTests
{
    private static readonly SmokeTestConfiguration Config = SmokeTestConfiguration.Load();

    [Fact]
    public async Task Health_endpoint_reports_ok()
    {
        using var client = CreateClient();
        var response = await SendAsync(() => client.GetAsync("health"));

        response.StatusCode.Should().Be(HttpStatusCode.OK);
        var body = await response.Content.ReadFromJsonAsync<HealthResponse>();
        body.Should().NotBeNull();
        body!.Status.Should().Be("ok");
    }

    [Fact]
    public async Task Schedule_endpoint_accepts_new_jobs()
    {
        using var client = CreateClient();
        var jobKey = $"1:dev:samples:smoke-{Guid.NewGuid():N}";
        var payload = new UpsertSchedulePayload(
            JobKey: jobKey,
            CronExpression: "0/5 * * * * ?",
            Description: "smoke-test",
            Metadata: new() { ["source"] = "Croniq.Api.Smoke" });

        var response = await SendAsync(() => client.PostAsJsonAsync("schedules", payload));

        response.StatusCode.Should().Be(HttpStatusCode.Created);
        var body = await response.Content.ReadFromJsonAsync<ScheduleResponse>();
        body.Should().NotBeNull();
        body!.TriggerId.Should().NotBeNullOrWhiteSpace();
        body.JobKey.Should().Be(jobKey);
        body.ScheduleExpression.Should().Be(payload.CronExpression);
    }

    private static HttpClient CreateClient()
    {
        var client = new HttpClient
        {
            BaseAddress = new Uri(Config.BaseUrl, UriKind.Absolute)
        };
        client.DefaultRequestHeaders.Add("X-Croniq-Key", Config.ApiKey);
        return client;
    }

    private static async Task<HttpResponseMessage> SendAsync(Func<Task<HttpResponseMessage>> action)
    {
        try
        {
            return await action().ConfigureAwait(false);
        }
        catch (HttpRequestException ex)
        {
            throw new InvalidOperationException(
                "Croniq.Api smoke endpoint is unreachable. Ensure docker compose up --build is running (see docs/technical/testing.md).",
                ex);
        }
    }

    private sealed record HealthResponse(string Status);

    private sealed record ScheduleResponse(string TriggerId, string JobKey, string ScheduleExpression);

    private sealed record UpsertSchedulePayload(
        string JobKey,
        string CronExpression,
        string? TriggerId = null,
        DateTimeOffset? StartAtUtc = null,
        DateTimeOffset? EndAtUtc = null,
        bool Enabled = true,
        string? Description = null,
        Dictionary<string, string>? Metadata = null);
}
