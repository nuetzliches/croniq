using Grpc.Core;

namespace Croniq.Rpc;

public static class SchedulerClientExtensions
{
    public static async Task<HealthResponse> HealthSafeAsync(this Scheduler.SchedulerClient client, HealthRequest request, CancellationToken cancellationToken = default)
    {
        try
        {
            return await client.HealthAsync(request, cancellationToken: cancellationToken).ConfigureAwait(false);
        }
        catch (RpcException ex)
        {
            throw CroniqRpcException.From(ex);
        }
    }

    public static async Task<TriggerJobResponse> TriggerJobSafeAsync(this Scheduler.SchedulerClient client, TriggerJobRequest request, CancellationToken cancellationToken = default)
    {
        try
        {
            return await client.TriggerJobAsync(request, cancellationToken: cancellationToken).ConfigureAwait(false);
        }
        catch (RpcException ex)
        {
            throw CroniqRpcException.From(ex);
        }
    }

    public static async Task<UpsertScheduleResponse> UpsertScheduleSafeAsync(this Scheduler.SchedulerClient client, UpsertScheduleRequest request, CancellationToken cancellationToken = default)
    {
        try
        {
            return await client.UpsertScheduleAsync(request, cancellationToken: cancellationToken).ConfigureAwait(false);
        }
        catch (RpcException ex)
        {
            throw CroniqRpcException.From(ex);
        }
    }

    public static async Task<DeleteScheduleResponse> DeleteScheduleSafeAsync(this Scheduler.SchedulerClient client, DeleteScheduleRequest request, CancellationToken cancellationToken = default)
    {
        try
        {
            return await client.DeleteScheduleAsync(request, cancellationToken: cancellationToken).ConfigureAwait(false);
        }
        catch (RpcException ex)
        {
            throw CroniqRpcException.From(ex);
        }
    }
}
