using System.Text.Json;
using System.Text.Json.Serialization;
using System.Linq;
using System.IO;

namespace Croniq.Runner;

internal sealed record OutboxEntry(
    string Id,
    string Type,
    JsonElement Payload,
    int Attempts,
    DateTimeOffset CreatedAt);

internal sealed class OutboxStore
{
    private static readonly JsonSerializerOptions SerializerOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
    };

    private readonly string _filePath;
    private readonly int _maxEntries;
    private readonly long _maxBytes;
    private readonly SemaphoreSlim _mutex = new(1, 1);
    private readonly List<OutboxEntry> _entries = [];

    public OutboxStore(string filePath, int maxEntries, long maxBytes)
    {
        _filePath = filePath;
        _maxEntries = Math.Max(1, maxEntries);
        _maxBytes = Math.Max(1024, maxBytes);
    }

    public IReadOnlyList<OutboxEntry> Snapshot()
    {
        lock (_entries)
        {
            return _entries.ToArray();
        }
    }

    public async Task LoadAsync(CancellationToken cancellationToken)
    {
        if (!File.Exists(_filePath))
        {
            return;
        }

        var lines = await File.ReadAllLinesAsync(_filePath, cancellationToken).ConfigureAwait(false);
        var loaded = new List<OutboxEntry>();
        foreach (var line in lines)
        {
            if (string.IsNullOrWhiteSpace(line))
            {
                continue;
            }

            try
            {
                var entry = JsonSerializer.Deserialize<OutboxEntry>(line, SerializerOptions);
                if (entry is not null)
                {
                    loaded.Add(entry);
                }
            }
            catch (JsonException)
            {
                // skip invalid entry
            }
        }

        lock (_entries)
        {
            _entries.Clear();
            _entries.AddRange(loaded);
        }
    }

    public async Task EnqueueAsync(string type, object payload, CancellationToken cancellationToken)
    {
        var entry = new OutboxEntry(
            Guid.NewGuid().ToString("N"),
            type,
            JsonSerializer.SerializeToElement(payload, SerializerOptions),
            Attempts: 0,
            CreatedAt: DateTimeOffset.UtcNow);

        await _mutex.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            _entries.Add(entry);
            Compact();
            await PersistAsync(cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _mutex.Release();
        }
    }

    public async Task MarkFailedAsync(string id, CancellationToken cancellationToken)
    {
        await _mutex.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            for (var i = 0; i < _entries.Count; i++)
            {
                if (!string.Equals(_entries[i].Id, id, StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                _entries[i] = _entries[i] with { Attempts = _entries[i].Attempts + 1 };
                break;
            }

            await PersistAsync(cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _mutex.Release();
        }
    }

    public async Task RemoveAsync(string id, CancellationToken cancellationToken)
    {
        await _mutex.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            _entries.RemoveAll(entry => string.Equals(entry.Id, id, StringComparison.OrdinalIgnoreCase));
            await PersistAsync(cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            _mutex.Release();
        }
    }

    private void Compact()
    {
        if (_entries.Count <= _maxEntries)
        {
            return;
        }

        var skip = _entries.Count - _maxEntries;
        _entries.RemoveRange(0, skip);
    }

    private async Task PersistAsync(CancellationToken cancellationToken)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(_filePath) ?? ".");
        var lines = _entries.Select(entry => JsonSerializer.Serialize(entry, SerializerOptions));
        await File.WriteAllLinesAsync(_filePath, lines, cancellationToken).ConfigureAwait(false);

        try
        {
            var info = new FileInfo(_filePath);
            if (info.Length <= _maxBytes)
            {
                return;
            }

            var trimCount = Math.Min(_entries.Count, Math.Max(1, (int)(_entries.Count * 0.1)));
            _entries.RemoveRange(0, trimCount);
            lines = _entries.Select(entry => JsonSerializer.Serialize(entry, SerializerOptions));
            await File.WriteAllLinesAsync(_filePath, lines, cancellationToken).ConfigureAwait(false);
        }
        catch
        {
            // ignore
        }
    }
}
