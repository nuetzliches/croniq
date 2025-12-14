using Grpc.Core;

namespace Croniq.Rpc;

public sealed class CroniqRpcException : Exception
{
    public CroniqRpcException(StatusCode statusCode, string? detail, RpcException inner)
        : base(detail ?? statusCode.ToString(), inner)
    {
        StatusCode = statusCode;
        Detail = detail ?? statusCode.ToString();
        Trailers = inner.Trailers;
    }

    public StatusCode StatusCode { get; }

    public string Detail { get; }

    public Metadata Trailers { get; }

    internal static CroniqRpcException From(RpcException ex) => new(ex.StatusCode, ex.Status.Detail, ex);
}
