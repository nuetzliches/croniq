using System.Net.Http.Headers;
using Grpc.Net.Client;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Rpc;

public sealed class CroniqSchedulerClientOptions
{
    public string Endpoint { get; set; } = string.Empty;
    public string? ApiKey { get; set; }
    public Action<HttpClient>? ConfigureHttpClient { get; set; }
    public Action<GrpcChannelOptions>? ConfigureChannel { get; set; }
}

public static class SchedulerClientServiceCollectionExtensions
{
    private const string ApiKeyHeader = "X-Croniq-Key";

    /// <summary>Registers the Scheduler gRPC client with sensible defaults (HTTP/2, API key header).</summary>
    public static IServiceCollection AddCroniqSchedulerClient(
        this IServiceCollection services,
        Action<CroniqSchedulerClientOptions> configure)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configure is null) throw new ArgumentNullException(nameof(configure));

        AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);

        var options = new CroniqSchedulerClientOptions();
        configure(options);

        if (string.IsNullOrWhiteSpace(options.Endpoint))
        {
            throw new InvalidOperationException("CroniqSchedulerClientOptions.Endpoint must be set.");
        }

        services.AddGrpcClient<Scheduler.SchedulerClient>(o =>
        {
            o.Address = new Uri(options.Endpoint);
        })
        .ConfigureHttpClient(client =>
        {
            client.DefaultRequestVersion = new Version(2, 0);
            client.DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher;
            if (!string.IsNullOrWhiteSpace(options.ApiKey))
            {
                client.DefaultRequestHeaders.Add(ApiKeyHeader, options.ApiKey);
            }

            options.ConfigureHttpClient?.Invoke(client);
        })
        .ConfigureChannel(o =>
        {
            options.ConfigureChannel?.Invoke(o);
        });

        return services;
    }
}
