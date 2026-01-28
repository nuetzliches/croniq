using System.Collections.Generic;

namespace Croniq.Runner;

public sealed record RunnerJobRegistration(
    string? Description = null,
    IReadOnlyDictionary<string, string>? Metadata = null);
