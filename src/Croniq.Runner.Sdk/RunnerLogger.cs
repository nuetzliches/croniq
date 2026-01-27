using Microsoft.Extensions.Logging;

namespace Croniq.Runner;

public interface IRunnerLogger
{
    void Info(string message, IReadOnlyDictionary<string, object?>? fields = null);
    void Warn(string message, IReadOnlyDictionary<string, object?>? fields = null);
    void Error(string message, IReadOnlyDictionary<string, object?>? fields = null);
}

internal sealed class LoggerAdapter : IRunnerLogger
{
    private readonly ILogger _logger;

    public LoggerAdapter(ILogger logger)
    {
        _logger = logger;
    }

    public void Info(string message, IReadOnlyDictionary<string, object?>? fields = null)
        => _logger.LogInformation("{Message} {@Fields}", message, fields ?? new Dictionary<string, object?>());

    public void Warn(string message, IReadOnlyDictionary<string, object?>? fields = null)
        => _logger.LogWarning("{Message} {@Fields}", message, fields ?? new Dictionary<string, object?>());

    public void Error(string message, IReadOnlyDictionary<string, object?>? fields = null)
        => _logger.LogError("{Message} {@Fields}", message, fields ?? new Dictionary<string, object?>());
}

internal sealed class ConsoleRunnerLogger : IRunnerLogger
{
    public void Info(string message, IReadOnlyDictionary<string, object?>? fields = null)
        => Console.WriteLine($"{message} {Format(fields)}");

    public void Warn(string message, IReadOnlyDictionary<string, object?>? fields = null)
        => Console.WriteLine($"warn: {message} {Format(fields)}");

    public void Error(string message, IReadOnlyDictionary<string, object?>? fields = null)
        => Console.WriteLine($"error: {message} {Format(fields)}");

    private static string Format(IReadOnlyDictionary<string, object?>? fields)
    {
        if (fields is null || fields.Count == 0)
        {
            return string.Empty;
        }

        return string.Join(", ", fields.Select(pair => $"{pair.Key}={pair.Value}"));
    }
}
