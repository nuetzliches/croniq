using System.Net.Http.Headers;
using Croniq.Rpc;
using Grpc.Net.Client;

static string Env(string key, string fallback)
{
    var value = Environment.GetEnvironmentVariable(key);
    return string.IsNullOrWhiteSpace(value) ? fallback : value.Trim();
}

var baseUrl = Env("CRONIQ_API_BASEURL", "http://localhost:5080");
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
    BaseAddress = new Uri(baseUrl),
    DefaultRequestVersion = new Version(2, 0),
    DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
};

if (!string.IsNullOrWhiteSpace(apiKey))
{
    httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
}

using var channel = GrpcChannel.ForAddress(baseUrl, new GrpcChannelOptions { HttpClient = httpClient });
var client = new Runner.RunnerClient(channel);

Console.WriteLine("Croniq gRPC runner (.NET)");
Console.WriteLine($"- base_url:    {baseUrl}");
Console.WriteLine($"- tenant_id:   {tenantId}");
Console.WriteLine($"- environment: {environment}");
Console.WriteLine($"- runner_id:   {runnerId}");

using var cts = new CancellationTokenSource();
Console.CancelKeyPress += (_, args) =>
{
    args.Cancel = true;
    cts.Cancel();
};

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
