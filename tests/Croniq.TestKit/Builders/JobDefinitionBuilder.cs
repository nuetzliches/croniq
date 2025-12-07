using System;
using System.Collections.Generic;
using Croniq.Persistence.Abstractions;

namespace Croniq.TestKit.Builders;

/// <summary>
/// Fluent helper for composing <see cref="JobDefinition"/> instances with sensible defaults.
/// </summary>
public sealed class JobDefinitionBuilder
{
    private string _jobKey = $"job:{Guid.NewGuid():N}";
    private string _namespace = "tests";
    private string _name = "job";
    private string? _variant;
    private string? _description = "integration job";
    private IReadOnlyDictionary<string, string>? _metadata;

    public JobDefinitionBuilder WithJobKey(string jobKey)
    {
        _jobKey = jobKey;
        return this;
    }

    public JobDefinitionBuilder WithNamespace(string @namespace)
    {
        _namespace = @namespace;
        return this;
    }

    public JobDefinitionBuilder WithName(string name)
    {
        _name = name;
        return this;
    }

    public JobDefinitionBuilder WithVariant(string? variant)
    {
        _variant = variant;
        return this;
    }

    public JobDefinitionBuilder WithDescription(string? description)
    {
        _description = description;
        return this;
    }

    public JobDefinitionBuilder WithMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        _metadata = metadata;
        return this;
    }

    public JobDefinition Build()
    {
        return new JobDefinition(_jobKey, _namespace, _name, _variant, _description, _metadata);
    }
}
