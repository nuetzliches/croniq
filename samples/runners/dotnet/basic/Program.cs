using Croniq.Runner;

static string Env(string key, string fallback)
{
    var value = Environment.GetEnvironmentVariable(key);
    return string.IsNullOrWhiteSpace(value) ? fallback : value.Trim();
}

static string? GetOptional(string key)
{
    var value = Environment.GetEnvironmentVariable(key);
    return string.IsNullOrWhiteSpace(value) ? null : value.Trim();
}

var runnerApiKey = Env("CRONIQ_RUNNER_DOTNET_API_KEY", string.Empty);
var apiKey = GetOptional("CRONIQ_API_KEY");
if (string.IsNullOrWhiteSpace(apiKey) && !string.IsNullOrWhiteSpace(runnerApiKey))
{
    Environment.SetEnvironmentVariable("CRONIQ_API_KEY", runnerApiKey);
}

var runnerId = GetOptional("CRONIQ_RUNNER_ID");
if (string.IsNullOrWhiteSpace(runnerId)
    || (string.Equals(runnerId, "default", StringComparison.OrdinalIgnoreCase)
        && !string.IsNullOrWhiteSpace(runnerApiKey)))
{
    Environment.SetEnvironmentVariable(
        "CRONIQ_RUNNER_ID",
        !string.IsNullOrWhiteSpace(runnerApiKey) ? "dotnet-default" : "default");
}

var config = RunnerConfig.FromEnvironment() with
{
    HeartbeatInterval = TimeSpan.FromSeconds(15)
};
var jobKey = Env("CRONIQ_JOB_KEY", "samples:dotnet-job");

var runner = new CroniqRunner(config);
runner.OnExecute(
    jobKey,
    async (context, payload, logger, cancellationToken) =>
    {
        logger.Info("execution started", new Dictionary<string, object?>
        {
            ["executionId"] = context.ExecutionId,
            ["jobKey"] = context.JobKey,
            ["triggerId"] = context.TriggerId,
            ["executionMode"] = context.ExecutionMode
        });

        await Task.Delay(100, cancellationToken);

        logger.Info("execution completed", new Dictionary<string, object?>
        {
            ["executionId"] = context.ExecutionId
        });
    },
    new RunnerJobRegistration(
        "Demo job registered by the .NET runner sample.",
        new Dictionary<string, string>
        {
            ["sample"] = "dotnet",
            ["sdk"] = "croniq-runner"
        }));

Console.WriteLine("Croniq runner (.NET)");
Console.WriteLine($"- base_url:    {config.BaseUrl}");
Console.WriteLine($"- grpc_url:    {config.GrpcBaseUrl ?? config.BaseUrl}");
Console.WriteLine($"- tenant_id:   {config.TenantId}");
Console.WriteLine($"- environment: {config.Environment}");
Console.WriteLine($"- runner_id:   {config.RunnerId}");
Console.WriteLine($"- job_key:     {jobKey}");

var shutdown = new TaskCompletionSource<string>();
Console.CancelKeyPress += (_, args) =>
{
    args.Cancel = true;
    shutdown.TrySetResult("SIGINT");
};

var runTask = runner.StartAsync();
var shutdownTask = shutdown.Task.ContinueWith(async _ =>
{
    Console.WriteLine("runner draining...");
    await runner.DrainAsync(TimeSpan.FromSeconds(30));
}, TaskScheduler.Default).Unwrap();

try
{
    await Task.WhenAny(runTask, shutdownTask);
    await runTask;
}
catch (RunnerIdInUseException ex)
{
    Console.Error.WriteLine($"runnerId already in use: {ex.Message}");
}
catch (RunnerJobRegistrationDeniedException ex)
{
    Console.Error.WriteLine($"job registration denied: {ex.Message}");
}
