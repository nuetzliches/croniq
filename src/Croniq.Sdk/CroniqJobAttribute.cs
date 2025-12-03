using System;

namespace Croniq.Sdk;

[AttributeUsage(AttributeTargets.Class, Inherited = false, AllowMultiple = false)]
public sealed class CroniqJobAttribute : Attribute
{
    public CroniqJobAttribute(string namespaceSegment, string jobName, string? variant = null)
    {
        NamespaceSegment = namespaceSegment ?? throw new ArgumentNullException(nameof(namespaceSegment));
        JobName = jobName ?? throw new ArgumentNullException(nameof(jobName));
        Variant = variant;
    }

    public string NamespaceSegment { get; }

    public string JobName { get; }

    public string? Variant { get; }
}
