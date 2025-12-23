using System;
using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Reflection;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Croniq.Webhooks;
using Croniq.Webhooks.Options;
using Microsoft.Extensions.Caching.Memory;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public class WebhookEndpointResolverTests
{
    private static readonly PartitionScope DefaultScope = new("tenant", "env");

    [Fact]
    public async Task ResolveAsync_UsesStoreAndCachesDescriptor()
    {
        var store = new StubWebhookStore();
        store.Definition = new WebhookEndpointDefinition(
            HookKey: "hook-a",
            JobKey: "ns:job",
            Secret: "fallback",
            Enabled: true,
            RequireSignature: true,
            RequestsPerMinute: 25,
            TenantId: "tenant",
            EnvironmentTag: "env",
            Metadata: new Dictionary<string, string> { ["source"] = "store" },
            IpRules: new[]
            {
                new WebhookIpRuleDefinition(1, "hook-a", "tenant", "env", "192.168.0.0/24", "local", null, DateTimeOffset.UtcNow, DateTimeOffset.UtcNow)
            },
            SignatureVersion: 1,
            CreatedAtUtc: DateTimeOffset.UtcNow,
            UpdatedAtUtc: DateTimeOffset.UtcNow);
        store.ActiveSecrets = new[]
        {
            new WebhookSecretMaterial("s1", "hash1", DateTime.UtcNow, null),
            new WebhookSecretMaterial("s1", "hash1", DateTime.UtcNow, null), // duplicate to ensure dedupe
            new WebhookSecretMaterial("s2", "hash2", DateTime.UtcNow, null)
        };

        var options = new CroniqWebhookOptions { RequestsPerMinute = 10 };
        var resolver = CreateResolver(store, options);

        var descriptor = await ResolveAsync(resolver, "hook-a", DefaultScope);
        descriptor.ShouldNotBeNull();
        Get<string>(descriptor, "JobKey").ShouldBe("ns:job");
        Get<int>(descriptor, "RequestsPerMinute").ShouldBe(25); // from definition

        var secrets = Get<IReadOnlyList<string>>(descriptor, "ActiveSecrets");
        secrets.ShouldBe(new[] { "s1", "s2" }, ignoreOrder: true);

        var allowedNetworks = Get<IReadOnlyList<object>>(descriptor, "AllowedNetworks");
        allowedNetworks.Count.ShouldBe(1);
        InvokeBool(descriptor, "IsIpAllowed", IPAddress.Parse("192.168.0.10")).ShouldBeTrue();
        InvokeBool(descriptor, "IsIpAllowed", IPAddress.Parse("10.0.0.1")).ShouldBeFalse();

        // cached retrieval
        var cached = Invoke<object?>(resolver, "TryGetCached", DefaultScope, "hook-a");
        cached.ShouldNotBeNull();

        // invalidation clears cache
        Invoke<object?>(resolver, "Invalidate", DefaultScope, "hook-a");
        Invoke<object?>(resolver, "TryGetCached", DefaultScope, "hook-a").ShouldBeNull();
    }

    [Fact]
    public async Task ResolveAsync_FallsBackToConfigAndNormalizesRequestsPerMinute()
    {
        var options = new CroniqWebhookOptions
        {
            RequestsPerMinute = 0, // will normalize to 1
            Security = new WebhookSecurityOptions { AllowUnsignedHooks = true }
        };
        options.Endpoints.Add(new WebhookEndpointOptions
        {
            HookKey = "cfg-hook",
            JobKey = "ns:job-cfg",
            RequireSignature = false,
            Secret = "cfg-secret",
            RequestsPerMinute = -5,
            Metadata = new Dictionary<string, string> { ["origin"] = "cfg" }
        });

        var resolver = CreateResolver(store: null, options);
        var descriptor = await ResolveAsync(resolver, "cfg-hook", DefaultScope);

        descriptor.ShouldNotBeNull();
        Get<int>(descriptor, "RequestsPerMinute").ShouldBe(1); // normalized from negative + default
        InvokeBool(descriptor, "IsIpAllowed", new object?[] { null! }).ShouldBeTrue(); // no IP rules -> allow all
    }

    [Fact]
    public void CacheNotifier_RemovesCachedEntry()
    {
        var cache = new MemoryCache(new MemoryCacheOptions());
        var notifierType = typeof(WebhookHostingExtensions).GetNestedType("WebhookEndpointCacheNotifier", BindingFlags.NonPublic)!;
        var notifier = Activator.CreateInstance(notifierType, cache)!;

        var cacheKey = GetCacheKey(DefaultScope, "hook-cache");
        cache.Set(cacheKey, new object());

        Invoke<object?>(notifier, "NotifyChanged", "hook-cache", DefaultScope);
        cache.TryGetValue(cacheKey, out _).ShouldBeFalse();
    }

    [Fact]
    public async Task CacheInvalidationService_UsesChangefeed()
    {
        var cache = new MemoryCache(new MemoryCacheOptions());
        var options = new CroniqWebhookOptions
        {
            Cache = new WebhookCacheOptions
            {
                ChangefeedEnabled = true,
                PollingIntervalSeconds = 0,
                BatchSize = 4
            }
        };

        var resolver = CreateResolver(store: null, options, cache);
        var cacheKey = GetCacheKey(DefaultScope, "hook-feed");
        cache.Set(cacheKey, new object());

        var changefeed = new StubChangefeed(new[]
        {
            new WebhookEndpointEvent(1, "hook-feed", "tenant", "env", "updated", DateTime.UtcNow, "actor", "corr")
        });

        var service = CreateCacheInvalidationService(resolver, options, changefeed);

        using var cts = new CancellationTokenSource(TimeSpan.FromMilliseconds(200));
        await service.StartAsync(cts.Token);
        await Task.Delay(50, cts.Token);
        await service.StopAsync(CancellationToken.None);

        cache.TryGetValue(cacheKey, out _).ShouldBeFalse();
    }

    private static object CreateResolver(IWebhookPersistenceProvider? store, CroniqWebhookOptions options, IMemoryCache? cache = null)
    {
        var resolverType = typeof(WebhookHostingExtensions).GetNestedType("WebhookEndpointResolver", BindingFlags.NonPublic)!;
        cache ??= new MemoryCache(new MemoryCacheOptions());
        var monitor = new StubOptionsMonitor<CroniqWebhookOptions>(options);
        return Activator.CreateInstance(resolverType, store, monitor, cache)!;
    }

    private static async Task<object?> ResolveAsync(object resolver, string hookKey, PartitionScope scope)
    {
        var task = (Task)Invoke<object?>(resolver, "ResolveAsync", hookKey, scope, CancellationToken.None)!;
        await task.ConfigureAwait(false);
        var resultProperty = task.GetType().GetProperty("Result", BindingFlags.Instance | BindingFlags.Public);
        return resultProperty?.GetValue(task);
    }

    private static T Get<T>(object target, string property)
    {
        var prop = target.GetType().GetProperty(property, BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic)!;
        return (T)prop.GetValue(target)!;
    }

    private static bool InvokeBool(object target, string method, params object?[] args)
        => (bool)Invoke<object>(target, method, args)!;

    private static T? Invoke<T>(object target, string method, params object?[] args)
    {
        var mi = target.GetType().GetMethod(method, BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic)!;
        return (T?)mi.Invoke(target, args);
    }

    private static string GetCacheKey(PartitionScope scope, string hookKey)
    {
        var buildKey = typeof(WebhookHostingExtensions)
            .GetMethod("BuildEndpointCacheKey", BindingFlags.Static | BindingFlags.NonPublic)!;
        return (string)buildKey.Invoke(null, new object[] { scope, hookKey })!;
    }

    private static BackgroundService CreateCacheInvalidationService(
        object resolver,
        CroniqWebhookOptions options,
        IWebhookEndpointChangefeed changefeed)
    {
        var serviceType = typeof(WebhookHostingExtensions).GetNestedType("WebhookEndpointCacheInvalidationService", BindingFlags.NonPublic)!;
        var loggerGeneric = typeof(TestLogger<>).MakeGenericType(serviceType);
        var logger = (ILogger)Activator.CreateInstance(loggerGeneric)!;
        var monitor = new StubOptionsMonitor<CroniqWebhookOptions>(options);
        return (BackgroundService)Activator.CreateInstance(serviceType, resolver, monitor, logger, changefeed)!;
    }

    private sealed class StubOptionsMonitor<T> : IOptionsMonitor<T>
    {
        public StubOptionsMonitor(T value) => CurrentValue = value;
        public T CurrentValue { get; private set; }
        public T Get(string? name) => CurrentValue;
        public IDisposable OnChange(Action<T, string> listener) => NullDisposable.Instance;
        private sealed class NullDisposable : IDisposable
        {
            public static readonly NullDisposable Instance = new();
            public void Dispose() { }
        }
    }

    private sealed class StubWebhookStore : IWebhookPersistenceProvider
    {
        public WebhookEndpointDefinition? Definition { get; set; }
        public IReadOnlyCollection<WebhookSecretMaterial> ActiveSecrets { get; set; } = Array.Empty<WebhookSecretMaterial>();

        public Task<WebhookEndpointDefinition?> FindByHookKeyAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
            => Task.FromResult<WebhookEndpointDefinition?>(Definition);

        public Task<IReadOnlyCollection<WebhookEndpointDefinition>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
            => Task.FromResult<IReadOnlyCollection<WebhookEndpointDefinition>>(Array.Empty<WebhookEndpointDefinition>());

        public Task UpsertAsync(WebhookEndpointUpsert request, CancellationToken cancellationToken)
            => Task.CompletedTask;

        public Task DeleteAsync(string hookKey, PartitionScope scope, bool hardDelete, CancellationToken cancellationToken)
            => Task.CompletedTask;

        public Task<WebhookSecretRotationResult> RotateSecretAsync(WebhookSecretRotate request, CancellationToken cancellationToken)
            => Task.FromResult(new WebhookSecretRotationResult(request.HookKey, "secret", "hash", DateTime.UtcNow, null));

        public Task<IReadOnlyCollection<WebhookSecretMaterial>> GetActiveSecretsAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
            => Task.FromResult(ActiveSecrets);

        public Task<IReadOnlyCollection<WebhookIpRuleDefinition>> ListIpRulesAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
            => Task.FromResult<IReadOnlyCollection<WebhookIpRuleDefinition>>(Array.Empty<WebhookIpRuleDefinition>());

        public Task<WebhookIpRuleDefinition> AddIpRuleAsync(WebhookIpRuleCreate request, CancellationToken cancellationToken)
            => Task.FromResult(new WebhookIpRuleDefinition(1, request.HookKey, request.TenantId, request.EnvironmentTag, request.Cidr, request.Description, request.CreatedBy, DateTimeOffset.UtcNow, DateTimeOffset.UtcNow));

        public Task DeleteIpRuleAsync(long ruleId, PartitionScope scope, string? deletedBy, string? correlationId, CancellationToken cancellationToken)
            => Task.CompletedTask;
    }

    private sealed class StubChangefeed : IWebhookEndpointChangefeed
    {
        private readonly IReadOnlyCollection<WebhookEndpointEvent> _events;
        public StubChangefeed(IReadOnlyCollection<WebhookEndpointEvent> events) => _events = events;

        public Task<IReadOnlyCollection<WebhookEndpointEvent>> FetchAsync(long afterEventId, int maxBatchSize, CancellationToken cancellationToken)
        {
            if (afterEventId >= 1)
            {
                return Task.FromResult<IReadOnlyCollection<WebhookEndpointEvent>>(Array.Empty<WebhookEndpointEvent>());
            }

            return Task.FromResult(_events);
        }
    }

    private sealed class TestLogger<T> : ILogger<T>
    {
        private sealed class NullScope : IDisposable
        {
            public static readonly NullScope Instance = new();
            public void Dispose() { }
        }

        IDisposable ILogger.BeginScope<TState>(TState state) => NullScope.Instance;
        bool ILogger.IsEnabled(LogLevel logLevel) => false;
        void ILogger.Log<TState>(LogLevel logLevel, EventId eventId, TState state, Exception? exception, Func<TState, Exception?, string> formatter)
        {
            // no-op
        }
    }
}
