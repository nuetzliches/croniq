using System.Diagnostics;
using System.Diagnostics.Metrics;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core;
using Shouldly;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using OpenTelemetry.Metrics;
using OpenTelemetry.Trace;
using Xunit;

namespace Croniq.Observability.Tests;

public sealed class ObservabilitySmokeTests : IAsyncLifetime
{
    private const string TestServiceName = "Croniq.Observability.Tests.Host";
    private const string TestTraceSource = "Croniq.Test.Trace";
    private const string TestMeterName = "Croniq.Test.Meter";

    private readonly OtlpHttpTestCollector _collector = new();

    public Task InitializeAsync() => _collector.StartAsync();

    public Task DisposeAsync() => _collector.DisposeAsync().AsTask();

    [Fact]
    public async Task AddCroniqObservability_exports_traces_metrics_and_logs()
    {
        var hostBuilder = Host.CreateApplicationBuilder();
        var services = hostBuilder.Services;
        var configuration = hostBuilder.Configuration;
        var loggingBuilder = hostBuilder.Logging;

        var endpoint = _collector.BaseAddress.ToString().TrimEnd('/');

        var otelBuilder = services.AddCroniqObservability(
            configuration,
            loggingBuilder,
            TestServiceName,
            options =>
            {
                options.EnableLogging = true;
                options.EnableMetrics = true;
                options.EnableTracing = true;
                options.OtlpEndpoint = endpoint;
                options.OtlpProtocol = "http";
                options.Environment = "ci";
                options.TenantId = "tests";
            });

        otelBuilder.WithTracing(tracing => tracing.AddSource(TestTraceSource));
        otelBuilder.WithMetrics(metrics => metrics.AddMeter(TestMeterName));

        var provider = services.BuildServiceProvider();
        var tracerProvider = provider.GetRequiredService<TracerProvider>();
        var meterProvider = provider.GetRequiredService<MeterProvider>();
        var loggerFactory = provider.GetRequiredService<ILoggerFactory>();

        EmitTestSpan(tracerProvider);
        EmitTestMetric(meterProvider);
        EmitTestLog(loggerFactory);

        await provider.DisposeAsync();

        (await _collector.WaitForTracesAsync(TimeSpan.FromSeconds(5))).ShouldBeTrue("span export should reach collector");
        (await _collector.WaitForMetricsAsync(TimeSpan.FromSeconds(5))).ShouldBeTrue("metric export should reach collector");
        (await _collector.WaitForLogsAsync(TimeSpan.FromSeconds(5))).ShouldBeTrue("log export should reach collector");

        _collector.LastTracePayloadLength.ShouldBeGreaterThan(0);
        _collector.LastMetricPayloadLength.ShouldBeGreaterThan(0);
        _collector.LastLogPayloadLength.ShouldBeGreaterThan(0);
    }

    private static void EmitTestSpan(TracerProvider tracerProvider)
    {
        using var source = new ActivitySource(TestTraceSource);
        using (var activity = source.StartActivity("test-span"))
        {
            activity?.SetTag("cron.test", true);
        }

        tracerProvider.ForceFlush();
    }

    private static void EmitTestMetric(MeterProvider meterProvider)
    {
        using var meter = new Meter(TestMeterName);
        var counter = meter.CreateCounter<long>("cronijob_test_metric");
        counter.Add(1);
        meterProvider.ForceFlush();
    }

    private static void EmitTestLog(ILoggerFactory loggerFactory)
    {
        var logger = loggerFactory.CreateLogger("Croniq.Test");
        logger.LogInformation("observability smoke log {Value}", 123);
    }
}

internal sealed class OtlpHttpTestCollector : IAsyncDisposable
{
    private readonly WebApplication _app;
    private readonly CancellationTokenSource _cts = new();
    private readonly SemaphoreSlim _traceSignal = new(0);
    private readonly SemaphoreSlim _metricSignal = new(0);
    private readonly SemaphoreSlim _logSignal = new(0);

    private volatile byte[]? _lastTracePayload;
    private volatile byte[]? _lastMetricPayload;
    private volatile byte[]? _lastLogPayload;

    public OtlpHttpTestCollector()
    {
        var port = GetFreeTcpPort();
        BaseAddress = new Uri($"http://127.0.0.1:{port}/");

        var builder = WebApplication.CreateBuilder();
        builder.WebHost.UseUrls(BaseAddress.ToString());
        builder.Logging.ClearProviders();

        _app = builder.Build();
        _app.MapPost("/v1/traces", async context =>
        {
            _lastTracePayload = await ReadPayloadAsync(context.Request.Body);
            _traceSignal.Release();
            context.Response.StatusCode = StatusCodes.Status200OK;
            await context.Response.CompleteAsync();
        });
        _app.MapPost("/v1/metrics", async context =>
        {
            _lastMetricPayload = await ReadPayloadAsync(context.Request.Body);
            _metricSignal.Release();
            context.Response.StatusCode = StatusCodes.Status200OK;
            await context.Response.CompleteAsync();
        });
        _app.MapPost("/v1/logs", async context =>
        {
            _lastLogPayload = await ReadPayloadAsync(context.Request.Body);
            _logSignal.Release();
            context.Response.StatusCode = StatusCodes.Status200OK;
            await context.Response.CompleteAsync();
        });
    }

    public Uri BaseAddress { get; }

    public int LastTracePayloadLength => _lastTracePayload?.Length ?? 0;
    public int LastMetricPayloadLength => _lastMetricPayload?.Length ?? 0;
    public int LastLogPayloadLength => _lastLogPayload?.Length ?? 0;

    public async Task StartAsync()
    {
        await _app.StartAsync(_cts.Token);
    }

    public async ValueTask DisposeAsync()
    {
        try
        {
            await _app.StopAsync();
        }
        finally
        {
            await _app.DisposeAsync();
            _cts.Cancel();
            _cts.Dispose();
            _traceSignal.Dispose();
            _metricSignal.Dispose();
            _logSignal.Dispose();
        }
    }

    public async Task<bool> WaitForTracesAsync(TimeSpan timeout) => await WaitForSignalAsync(_traceSignal, timeout);

    public async Task<bool> WaitForMetricsAsync(TimeSpan timeout) => await WaitForSignalAsync(_metricSignal, timeout);

    public async Task<bool> WaitForLogsAsync(TimeSpan timeout) => await WaitForSignalAsync(_logSignal, timeout);

    private static async Task<byte[]> ReadPayloadAsync(Stream stream)
    {
        using var memory = new MemoryStream();
        await stream.CopyToAsync(memory);
        return memory.ToArray();
    }

    private static async Task<bool> WaitForSignalAsync(SemaphoreSlim signal, TimeSpan timeout)
    {
        using var timeoutCts = new CancellationTokenSource(timeout);
        try
        {
            await signal.WaitAsync(timeoutCts.Token);
            return true;
        }
        catch (OperationCanceledException)
        {
            return false;
        }
    }

    private static int GetFreeTcpPort()
    {
        using var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        var port = ((IPEndPoint)listener.LocalEndpoint).Port;
        listener.Stop();
        return port;
    }
}
