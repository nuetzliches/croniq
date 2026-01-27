using Croniq.Runner;

static string Env(string key, string fallback)
{
    var value = Environment.GetEnvironmentVariable(key);
    return string.IsNullOrWhiteSpace(value) ? fallback : value.Trim();
}

var config = RunnerConfig.FromEnvironment() with
{
    HeartbeatInterval = TimeSpan.FromSeconds(15)
};
var jobKey = Env("CRONIQ_JOB_KEY", "demo-job");

var runner = new CroniqRunner(config);
runner.OnExecute(jobKey, async (context, payload, logger, cancellationToken) =>
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
});

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
