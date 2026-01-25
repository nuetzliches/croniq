using System;

namespace Croniq.Core.Hosting;

public sealed record WorkerDispatchStatus(
    bool GrpcConnected,
    DateTimeOffset? LastConnectedAtUtc,
    DateTimeOffset? LastFallbackAtUtc);

public interface IWorkerDispatchStatusProvider
{
    WorkerDispatchStatus GetStatus();
}

public sealed class NoOpWorkerDispatchStatusProvider : IWorkerDispatchStatusProvider
{
    public WorkerDispatchStatus GetStatus() => new(false, null, null);
}