using System;
using System.Linq;
using System.Reflection;
using System.Reflection.Emit;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Core.Hosting;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests;

public class ServiceCollectionExtensionsTests
{
    [Fact]
    public void Registers_core_services_and_jobs()
    {
        var services = new ServiceCollection();

        services.AddCroniqCore(options =>
        {
            options.EnvironmentTag = "dev";
        });
        services.AddLogging();
        services.AddSingleton<StubJobStore>();
        services.AddSingleton<IJobStore>(sp => sp.GetRequiredService<StubJobStore>());
        services.AddSingleton<IJobPersistenceProvider>(sp => sp.GetRequiredService<StubJobStore>());
        services.AddCroniqJob<SampleJob>();

        var provider = services.BuildServiceProvider();

        provider.GetRequiredService<TriggerWorker>().ShouldNotBeNull();
        provider.GetRequiredService<IJobExecutionPipeline>().ShouldBeOfType<DefaultJobExecutionPipeline>();
        provider.GetRequiredService<IJobTrigger>().ShouldBeOfType<DefaultJobTrigger>();
        provider.GetRequiredService<IMisfirePolicy>().ShouldBeOfType<DefaultMisfirePolicy>();
        provider.GetRequiredService<IJobRegistry>().TryGet(JobKey.Create("core", "sample"), out _).ShouldBeTrue();
    }

    [Fact]
    public void AddCroniqJob_throws_when_attribute_missing()
    {
        var services = new ServiceCollection();
        Should.Throw<InvalidOperationException>(() => services.AddCroniqJob<JobWithoutAttribute>());
    }

    [Fact]
    public void AddCroniqFileExecutionLogStore_replaces_noop_services_with_file_implementations()
    {
        var services = new ServiceCollection();

        services.AddCroniqCore();
        services.AddCroniqFileExecutionLogStore(options => options.BasePath = "my-logs");

        var provider = services.BuildServiceProvider();

        provider.GetRequiredService<IExecutionLogStore>().ShouldBeOfType<FileExecutionLogStore>();
        provider.GetRequiredService<IExecutionLogReader>().ShouldBeOfType<FileExecutionLogReader>();
        provider.GetRequiredService<IExecutionHistoryReader>().ShouldBeOfType<FileExecutionHistoryReader>();
        provider.GetRequiredService<FileExecutionLogStoreOptions>().BasePath.ShouldBe("my-logs");
    }

    [Fact]
    public void AddCroniqExecutionLogSink_registers_provider_and_applies_options_when_configured()
    {
        var services = new ServiceCollection();

        services.AddCroniqCore();

        services.AddLogging(builder => builder.AddCroniqExecutionLogSink(options =>
        {
            options.MinimumLevel = LogLevel.Warning;
            options.BatchSize = 7;
        }));

        var provider = services.BuildServiceProvider();
        provider.GetServices<ILoggerProvider>().Any(p => p is ExecutionLogSinkProvider).ShouldBeTrue();

        var options = provider.GetRequiredService<Microsoft.Extensions.Options.IOptions<ExecutionLogSinkOptions>>();
        options.Value.MinimumLevel.ShouldBe(LogLevel.Warning);
        options.Value.BatchSize.ShouldBe(7);
    }

    [Fact]
    public void AddCroniqWorkerHost_registers_hosted_service_and_configures_options()
    {
        var services = new ServiceCollection();

        services.AddCroniqCore();
        services.AddLogging();
        services.AddSingleton<IJobStore, StubJobStore>();

        services.AddCroniqWorkerHost(options => options.BatchSize = 123);

        var provider = services.BuildServiceProvider();
        provider.GetServices<IHostedService>().Any(s => s is CroniqWorkerHostedService).ShouldBeTrue();
        provider.GetServices<IHostedService>().Any(s => s is CroniqWorkerHeartbeatHostedService).ShouldBeTrue();
        provider.GetRequiredService<Microsoft.Extensions.Options.IOptions<WorkerHostOptions>>().Value.BatchSize.ShouldBe(123);
    }

    [Fact]
    public void AddCroniqJobsFromAssembly_registers_scanned_jobs()
    {
        var services = new ServiceCollection();
        services.AddCroniqCore(options =>
        {
            options.EnvironmentTag = "dev";
        });
        services.AddLogging();
        services.AddSingleton<StubJobStore>();
        services.AddSingleton<IJobStore>(sp => sp.GetRequiredService<StubJobStore>());
        services.AddSingleton<IJobPersistenceProvider>(sp => sp.GetRequiredService<StubJobStore>());

        var assembly = BuildDynamicJobAssembly(("scan", "job", null));
        services.AddCroniqJobsFromAssembly(assembly);

        var provider = services.BuildServiceProvider();
        var registry = provider.GetRequiredService<IJobRegistry>();
        registry.TryGet(JobKey.Create("scan", "job"), out _).ShouldBeTrue();
    }

    [CroniqJob("core", "sample")]
    private sealed class SampleJob : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default) => Task.CompletedTask;
    }

    private sealed class JobWithoutAttribute : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default) => Task.CompletedTask;
    }

    private static Assembly BuildDynamicJobAssembly(params (string NamespaceSegment, string JobName, string? Variant)[] specs)
    {
        var assemblyName = new AssemblyName($"Croniq.Dynamic.{Guid.NewGuid():N}");
        var assemblyBuilder = AssemblyBuilder.DefineDynamicAssembly(assemblyName, AssemblyBuilderAccess.Run);
        var moduleBuilder = assemblyBuilder.DefineDynamicModule("Main");
        var jobInterface = typeof(IJob);
        var executeMethod = jobInterface.GetMethod(nameof(IJob.ExecuteAsync));
        var attributeConstructor = typeof(CroniqJobAttribute).GetConstructor(new[] { typeof(string), typeof(string), typeof(string) });
        var completedTaskGetter = typeof(Task).GetProperty(nameof(Task.CompletedTask), BindingFlags.Public | BindingFlags.Static)?.GetGetMethod();

        foreach (var (namespaceSegment, jobName, variant) in specs)
        {
            var typeBuilder = moduleBuilder.DefineType($"DynamicJob_{Guid.NewGuid():N}", TypeAttributes.Public | TypeAttributes.Class);
            typeBuilder.AddInterfaceImplementation(jobInterface);

            if (attributeConstructor is null)
            {
                throw new InvalidOperationException("CroniqJobAttribute constructor not found.");
            }

            typeBuilder.SetCustomAttribute(new CustomAttributeBuilder(attributeConstructor, new object?[] { namespaceSegment, jobName, variant }));

            var methodBuilder = typeBuilder.DefineMethod(
                nameof(IJob.ExecuteAsync),
                MethodAttributes.Public | MethodAttributes.Virtual,
                typeof(Task),
                new[] { typeof(IJobExecutionContext), typeof(CancellationToken) });

            var il = methodBuilder.GetILGenerator();
            if (completedTaskGetter is null)
            {
                throw new InvalidOperationException("Task.CompletedTask getter not found.");
            }

            il.Emit(OpCodes.Call, completedTaskGetter);
            il.Emit(OpCodes.Ret);

            if (executeMethod is not null)
            {
                typeBuilder.DefineMethodOverride(methodBuilder, executeMethod);
            }

            _ = typeBuilder.CreateType();
        }

        return assemblyBuilder;
    }

    private sealed class StubJobStore : IJobPersistenceProvider
    {
        public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken) =>
            Task.FromResult<IReadOnlyCollection<TriggerLease>>(Array.Empty<TriggerLease>());

        public Task<TriggerLease?> TryRenewLeaseAsync(TriggerLeaseRenewRequest request, CancellationToken cancellationToken) =>
            Task.FromResult<TriggerLease?>(null);

        public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken) => Task.CompletedTask;

        public Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken) => Task.CompletedTask;

        public Task UpsertJobAsync(JobDefinition job, PartitionScope scope, CancellationToken cancellationToken) => Task.CompletedTask;

        public Task<IReadOnlyCollection<JobDefinition>> ListJobsAsync(PartitionScope scope, CancellationToken cancellationToken) =>
            Task.FromResult<IReadOnlyCollection<JobDefinition>>(Array.Empty<JobDefinition>());

        public Task<JobDefinition?> GetJobAsync(string jobKey, PartitionScope scope, CancellationToken cancellationToken) =>
            Task.FromResult<JobDefinition?>(null);

        public Task DeleteJobAsync(string jobKey, PartitionScope scope, CancellationToken cancellationToken) => Task.CompletedTask;

        public Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken) => Task.CompletedTask;

        public Task<IReadOnlyCollection<TriggerDefinition>> ListTriggersAsync(PartitionScope scope, CancellationToken cancellationToken) =>
            Task.FromResult<IReadOnlyCollection<TriggerDefinition>>(Array.Empty<TriggerDefinition>());

        public Task DeleteTriggerAsync(string triggerId, PartitionScope scope, CancellationToken cancellationToken) => Task.CompletedTask;
    }
}
