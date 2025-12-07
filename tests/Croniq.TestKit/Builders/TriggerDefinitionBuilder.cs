using System;
using System.Collections.Generic;
using Croniq.Persistence.Abstractions;

namespace Croniq.TestKit.Builders;

/// <summary>
/// Fluent helper for composing <see cref="TriggerDefinition"/> instances with predictable defaults.
/// </summary>
public sealed class TriggerDefinitionBuilder
{
    private string _triggerId = $"trigger:{Guid.NewGuid():N}";
    private string _jobKey = $"job:{Guid.NewGuid():N}";
    private string _schedule = "0/1 * * * * ?";
    private PartitionScope _scope = new("1", "dev");
    private DateTimeOffset? _startAtUtc;
    private DateTimeOffset? _endAtUtc;
    private bool _enabled = true;
    private IReadOnlyDictionary<string, string>? _metadata;

    public TriggerDefinitionBuilder WithTriggerId(string triggerId)
    {
        _triggerId = triggerId;
        return this;
    }

    public TriggerDefinitionBuilder WithJobKey(string jobKey)
    {
        _jobKey = jobKey;
        return this;
    }

    public TriggerDefinitionBuilder WithSchedule(string expression)
    {
        _schedule = expression;
        return this;
    }

    public TriggerDefinitionBuilder WithScope(PartitionScope scope)
    {
        _scope = scope;
        return this;
    }

    public TriggerDefinitionBuilder StartingAt(DateTimeOffset? startAtUtc)
    {
        _startAtUtc = startAtUtc;
        return this;
    }

    public TriggerDefinitionBuilder EndingAt(DateTimeOffset? endAtUtc)
    {
        _endAtUtc = endAtUtc;
        return this;
    }

    public TriggerDefinitionBuilder Enabled(bool enabled)
    {
        _enabled = enabled;
        return this;
    }

    public TriggerDefinitionBuilder WithMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        _metadata = metadata;
        return this;
    }

    public TriggerDefinition Build()
    {
        return new TriggerDefinition(_triggerId, _jobKey, _schedule, _scope, _startAtUtc, _endAtUtc, _enabled, _metadata);
    }
}
