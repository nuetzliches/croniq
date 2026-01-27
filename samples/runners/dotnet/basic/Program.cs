using System.Net.Http.Headers;
using System.Text.Json;
using Croniq.Rpc;
using Grpc.Net.Client;

static string Env(string key, string fallback)
{
    var value = Environment.GetEnvironmentVariable(key);
    return string.IsNullOrWhiteSpace(value) ? fallback : value.Trim();
}

var baseUrl = Env("CRONIQ_API_BASEURL", "http://localhost:5080");
var grpcBaseUrl = Env("CRONIQ_GRPC_BASEURL", baseUrl);
var tenantId = Env("CRONIQ_TENANT_ID", "default");
var environment = Env("CRONIQ_ENVIRONMENT", "dev");
var apiKey = Env("CRONIQ_API_KEY", string.Empty);
var runnerId = Env("CRONIQ_RUNNER_ID", "default");
var allowTestExecutions = string.Equals(Env("CRONIQ_ALLOW_TEST_EXECUTIONS", "false"), "true", StringComparison.OrdinalIgnoreCase);
var maxInflight = int.TryParse(Env("CRONIQ_MAX_INFLIGHT", "1"), out var parsedInflight)
    ? Math.Max(parsedInflight, 1)
    : 1;

AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);

using var httpClient = new HttpClient
{
    BaseAddress = new Uri(grpcBaseUrl),
    DefaultRequestVersion = new Version(2, 0),
    DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
};

if (!string.IsNullOrWhiteSpace(apiKey))
{
    httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
}

using var channel = GrpcChannel.ForAddress(grpcBaseUrl, new GrpcChannelOptions { HttpClient = httpClient });
var client = new Runner.RunnerClient(channel);

using var heartbeatClient = new HttpClient
{
    BaseAddress = new Uri(baseUrl)
};

if (!string.IsNullOrWhiteSpace(apiKey))
{
    heartbeatClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
}

Console.WriteLine("Croniq gRPC runner (.NET)");
Console.WriteLine($"- base_url:    {baseUrl}");
Console.WriteLine($"- grpc_url:    {grpcBaseUrl}");
Console.WriteLine($"- tenant_id:   {tenantId}");
Console.WriteLine($"- environment: {environment}");
Console.WriteLine($"- runner_id:   {runnerId}");

using var cts = new CancellationTokenSource();
Console.CancelKeyPress += (_, args) =>
{
    args.Cancel = true;
    cts.Cancel();
};

var heartbeatTask = Task.Run(async () =>
{
    var path = $"/tenants/{Uri.EscapeDataString(tenantId)}/runners/heartbeat";
    while (!cts.IsCancellationRequested)
    {
        var payload = new
        {
            environmentTag = environment,
            runnerId,
            seenAtUtc = DateTimeOffset.UtcNow
        };

        try
        {
            using var request = new HttpRequestMessage(HttpMethod.Post, path)
            {
                Content = new StringContent(JsonSerializer.Serialize(payload), System.Text.Encoding.UTF8, "application/json")
            };
            await heartbeatClient.SendAsync(request, cts.Token).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (cts.IsCancellationRequested)
        {
            break;
        }
        catch
        {
            // Best-effort heartbeat; failures are logged by the server or retried next interval.
        }

        try
        {
            await Task.Delay(TimeSpan.FromSeconds(15), cts.Token).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            break;
        }
    }
}, cts.Token);

using var call = client.Connect(cancellationToken: cts.Token);
await call.RequestStream.WriteAsync(new RunnerMessage
{
    Hello = new RunnerHello
    {
        RunnerId = runnerId,
        MaxInflight = maxInflight,
        AllowTestExecutions = allowTestExecutions
    }
});

while (await call.ResponseStream.MoveNext(cts.Token))
{
    var message = call.ResponseStream.Current;
    if (message is null)
    {
        continue;
    }

    if (message.Hello is not null)
    {
        Console.WriteLine($"connected: tenant={message.Hello.TenantId} env={message.Hello.EnvironmentTag}");
        continue;
    }

    if (message.Assigned is null)
    {
        continue;
    }

    var lease = message.Assigned;
    Console.WriteLine($"claimed lease: jobKey={lease.JobKey} triggerId={lease.TriggerId} leaseId={lease.LeaseId}");
    if (!string.IsNullOrWhiteSpace(lease.ExecutionMode) || !string.IsNullOrWhiteSpace(lease.InvocationSource))
    {
        var mode = string.IsNullOrWhiteSpace(lease.ExecutionMode) ? "normal" : lease.ExecutionMode;
        var source = string.IsNullOrWhiteSpace(lease.InvocationSource) ? "schedule" : lease.InvocationSource;
        Console.WriteLine($"- intent: mode={mode} source={source}");
    }

    await call.RequestStream.WriteAsync(new RunnerMessage
    {
        Events = new WorkEvents
        {
            ExecutionId = lease.ExecutionId,
            LeaseId = lease.LeaseId,
            Events =
            {
                new WorkEvent
                {
                    Message = $"processing execution {lease.ExecutionId}",
                    Level = "Information",
                    EventType = "runner"
                }
            }
        }
    });

    await call.RequestStream.WriteAsync(new RunnerMessage
    {
        AckSuccess = new WorkAckSuccess
        {
            ExecutionId = lease.ExecutionId,
            LeaseId = lease.LeaseId
        }
    });

    Console.WriteLine($"acked lease: leaseId={lease.LeaseId}");
}

await call.RequestStream.CompleteAsync();
try
{
    await heartbeatTask.ConfigureAwait(false);
}
catch
{
    // ignore heartbeat failures on shutdown
}
