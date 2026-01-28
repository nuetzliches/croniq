using System;

namespace Croniq.Runner;

public abstract class RunnerException : Exception
{
    protected RunnerException(string message) : base(message) { }
    protected RunnerException(string message, Exception innerException) : base(message, innerException) { }
}

public sealed class RunnerMismatchException : RunnerException
{
    public RunnerMismatchException(string message) : base(message) { }
    public RunnerMismatchException(string message, Exception innerException) : base(message, innerException) { }
}

public sealed class RunnerIdInUseException : RunnerException
{
    public RunnerIdInUseException(string message) : base(message) { }
    public RunnerIdInUseException(string message, Exception innerException) : base(message, innerException) { }
}

public sealed class RunnerJobRegistrationDeniedException : RunnerException
{
    public RunnerJobRegistrationDeniedException(string message) : base(message) { }
    public RunnerJobRegistrationDeniedException(string message, Exception innerException) : base(message, innerException) { }
}

public sealed class LeaseConflictException : RunnerException
{
    public LeaseConflictException(string message) : base(message) { }
    public LeaseConflictException(string message, Exception innerException) : base(message, innerException) { }
}
