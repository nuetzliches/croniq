using System;
using System.IO;
using System.Net.Sockets;
using System.Threading;
using Grpc.Core;
using Microsoft.AspNetCore.Connections;

namespace Croniq.Api;

internal static class GrpcDisconnects
{
    public static bool IsExpected(Exception exception, CancellationToken cancellationToken)
    {
        if (cancellationToken.IsCancellationRequested && exception is OperationCanceledException)
        {
            return true;
        }

        if (exception is RpcException rpcException
            && (rpcException.StatusCode == StatusCode.Cancelled
                || rpcException.StatusCode == StatusCode.Unavailable
                || rpcException.StatusCode == StatusCode.Aborted))
        {
            return true;
        }

        if (exception is IOException ioException && IsConnectionAborted(ioException))
        {
            return true;
        }

        if (exception is IOException ioExceptionWithMessage && IsStreamReset(ioExceptionWithMessage))
        {
            return true;
        }

        if (exception is AggregateException aggregateException
            && aggregateException.InnerExceptions.Count == 1)
        {
            return IsExpected(aggregateException.InnerExceptions[0], cancellationToken);
        }

        return false;
    }

    private static bool IsConnectionAborted(IOException exception)
    {
        if (exception.InnerException is ConnectionAbortedException
            or ConnectionResetException)
        {
            return true;
        }

        if (exception.InnerException is SocketException socketException
            && socketException.SocketErrorCode == SocketError.ConnectionReset)
        {
            return true;
        }

        return false;
    }

    private static bool IsStreamReset(IOException exception)
    {
        if (string.IsNullOrWhiteSpace(exception.Message))
        {
            return false;
        }

        return exception.Message.Contains("request stream", StringComparison.OrdinalIgnoreCase)
            || exception.Message.Contains("client reset", StringComparison.OrdinalIgnoreCase)
            || exception.Message.Contains("stream reset", StringComparison.OrdinalIgnoreCase);
    }
}
