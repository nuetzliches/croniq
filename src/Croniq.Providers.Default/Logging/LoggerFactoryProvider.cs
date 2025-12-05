using System;
using Croniq.Providers.Logging;
using Microsoft.Extensions.Logging;

namespace Croniq.Providers.Default.Logging;

/// <summary>
/// Default logging provider that wraps an <see cref="ILoggerFactory"/>.
/// </summary>
public sealed class LoggerFactoryProvider : ILoggingProvider
{
    private readonly ILoggerFactory _factory;

    public LoggerFactoryProvider(ILoggerFactory factory)
    {
        _factory = factory ?? throw new ArgumentNullException(nameof(factory));
    }

    public ILogger CreateLogger(string categoryName)
    {
        if (string.IsNullOrWhiteSpace(categoryName)) throw new ArgumentException("Category name is required.", nameof(categoryName));
        return _factory.CreateLogger(categoryName);
    }
}
