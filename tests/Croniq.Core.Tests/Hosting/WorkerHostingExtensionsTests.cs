using System;
using System.Collections.Generic;
using Croniq.Data.SqlServer;
using Croniq.Hosting;
using Croniq.Options;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Hosting;

public class WorkerHostingExtensionsTests
{
    [Fact]
    public void AddCroniqWorkerServices_Throws_OnNullInputs()
    {
        var config = new ConfigurationBuilder().Build();

        Should.Throw<ArgumentNullException>(() => WorkerHostingExtensions.AddCroniqWorkerServices(null!, config));
        Should.Throw<ArgumentNullException>(() => new ServiceCollection().AddCroniqWorkerServices(null!));
    }

    [Fact]
    public void AddCroniqWorkerServices_Throws_WhenSqlServerModeMissingConnectionString()
    {
        var services = new ServiceCollection();
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Persistence:Mode"] = "SqlServer"
            })
            .Build();

        Should.Throw<InvalidOperationException>(() => services.AddCroniqWorkerServices(config));
    }

    [Fact]
    public void AddCroniqWorkerServices_PrefersPersistenceConnectionString()
    {
        var services = new ServiceCollection();
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Persistence:Mode"] = "SqlServer",
                ["Croniq:Persistence:SqlServer:ConnectionString"] = "Server=primary;Database=Croniq;",
                ["Croniq:SqlServer:ConnectionString"] = "Server=secondary;Database=Croniq;"
            })
            .Build();

        services.AddCroniqWorkerServices(config);
        var provider = services.BuildServiceProvider();

        var options = provider.GetRequiredService<IOptions<SqlServerOptions>>().Value;
        options.ConnectionString.ShouldBe("Server=primary;Database=Croniq;");
    }

    [Fact]
    public void AddCroniqWorkerServices_binds_worker_dispatch_options()
    {
        var services = new ServiceCollection();
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:WorkerDispatch:EnableGrpc"] = "true",
                ["Croniq:WorkerDispatch:GrpcEndpoint"] = "http://localhost:5005",
                ["Croniq:WorkerDispatch:ApiKey"] = "ak_worker"
            })
            .Build();

        services.AddCroniqWorkerServices(config);
        var provider = services.BuildServiceProvider();

        provider.GetRequiredService<IOptions<WorkerDispatchOptions>>().Value.EnableGrpc.ShouldBeTrue();
    }
}
