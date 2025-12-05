using System;
using Microsoft.Extensions.Logging;

namespace Croniq.Providers.Logging;

/// <summary>
/// Abstraction for supplying loggers to Croniq components and jobs.
/// </summary>
public interface ILoggingProvider
{
    ILogger CreateLogger(string categoryName);

    ILogger<T> CreateLogger<T>() => (ILogger<T>)CreateLogger(typeof(T).FullName ?? typeof(T).Name);
}
