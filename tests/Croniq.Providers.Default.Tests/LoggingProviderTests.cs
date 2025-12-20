using Croniq.Providers.Logging;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Providers.Default.Tests;

public class LoggingProviderTests
{
    [Fact]
    public void CreateLoggerOfT_UsesTypeFullName()
    {
        var provider = new RecordingLoggingProvider();

        ((ILoggingProvider)provider).CreateLogger<RecordingLoggingProvider>();

        provider.LastCategory.ShouldBe(typeof(RecordingLoggingProvider).FullName);
    }

    private sealed class RecordingLoggingProvider : ILoggingProvider
    {
        public string? LastCategory { get; private set; }

        public ILogger CreateLogger(string categoryName)
        {
            LastCategory = categoryName;
            return NullLogger<RecordingLoggingProvider>.Instance;
        }
    }
}
