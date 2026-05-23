namespace Croniq.Runner.Sdk;

/// <summary>
/// Thrown by job handlers to signal a controlled failure. The SDK catches
/// any exception (including this one) from a handler and reports
/// <c>status=failure</c> with the exception's message on the ack call.
/// </summary>
public class CroniqHandlerException : Exception
{
    public CroniqHandlerException(string message)
        : base(message)
    {
    }

    public CroniqHandlerException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}

/// <summary>
/// Thrown when an execution arrives for a job key with no registered
/// handler and no default handler. Surfaces to the server as a failed
/// execution so operators can spot misconfigured routing.
/// </summary>
public sealed class NoHandlerRegisteredException(string jobKey)
    : CroniqHandlerException($"no handler registered for job_key '{jobKey}'")
{
    /// <summary>The job key for which no handler was found.</summary>
    public string JobKey { get; } = jobKey;
}
