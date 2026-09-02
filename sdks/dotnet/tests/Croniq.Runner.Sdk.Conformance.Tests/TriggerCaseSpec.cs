namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// In-memory representation of a single <b>trigger (producer)</b> conformance
/// case from <c>cases-trigger/</c>. Mirrors
/// <c>schema/trigger-case-schema.json</c>; all fields use snake_case to match
/// the YAML.
/// </summary>
/// <remarks>
/// Deliberately separate from <see cref="CaseSpec"/> rather than sharing its
/// expectation types (issue #554). A producer case declares
/// <c>trigger_config</c> + <c>trigger_calls</c> where a runner case declares
/// <c>runner_config</c> + <c>handlers</c>, and it adds a <c>body_absent</c>
/// assertion the runner corpus does not use.
///
/// Keeping them apart preserves the loader's strictness in both directions:
/// adding <c>BodyAbsent</c> to the shared <see cref="HttpExpectation"/> would
/// let a <i>runner</i> case declare it and load cleanly while
/// <see cref="ConformanceRunner"/> never asserted it — a green suite for an
/// unenforced contract, exactly the failure mode #460 closed.
///
/// <see cref="ScriptEntrySpec"/> and <see cref="RespondSpec"/> <i>are</i>
/// shared: the two schemas define <c>server_script</c> identically, and
/// <see cref="MockServerHarness"/> already consumes that shape.
/// </remarks>
public sealed class TriggerCaseSpec
{
    public string Name { get; set; } = "";
    public string? Description { get; set; }
    public TriggerConfigSpec TriggerConfig { get; set; } = new();
    public List<TriggerCallSpec> TriggerCalls { get; set; } = [];
    public List<ScriptEntrySpec> ServerScript { get; set; } = [];
    public TriggerExpectationsSpec Expectations { get; set; } = new();
}

/// <summary>
/// Maps to the trigger client's options. <c>server_url</c> is intentionally
/// absent — the binding injects the mock server's base URL, exactly as runner
/// cases omit it from <c>runner_config</c>.
/// </summary>
public sealed class TriggerConfigSpec
{
    public string? ApiKey { get; set; }
    public string? BearerToken { get; set; }
}

/// <summary>One <c>trigger(...)</c> invocation and the outcome it must produce.</summary>
public sealed class TriggerCallSpec
{
    public TriggerCallRequestSpec Request { get; set; } = new();
    public TriggerCallExpectSpec Expect { get; set; } = new();
}

/// <summary>
/// Arguments handed to the trigger client. A field absent here must not appear
/// in the outbound JSON body — asserted via
/// <see cref="TriggerHttpExpectation.BodyAbsent"/>.
/// </summary>
/// <remarks>
/// The reference types stay nullable so "absent in the YAML" and "present but
/// empty" remain distinguishable at this layer: case
/// <c>12-trigger-empty-optionals</c> passes empty values deliberately (issue
/// #553), and collapsing them into defaults here would make that case vacuous.
/// </remarks>
public sealed class TriggerCallRequestSpec
{
    public string JobKey { get; set; } = "";
    public List<string>? Require { get; set; }
    public List<string>? Prefer { get; set; }
    public Dictionary<string, object?>? Metadata { get; set; }
    public string? Timeout { get; set; }
    public string? IdempotencyKey { get; set; }
}

/// <summary>
/// The outcome the client must surface. By convention exactly one of
/// <see cref="Response"/> (call succeeds) or <see cref="Error"/> (call throws).
/// </summary>
public sealed class TriggerCallExpectSpec
{
    public TriggerExpectedResponseSpec? Response { get; set; }
    public bool? Error { get; set; }
}

/// <summary>
/// Subset match on the parsed result the client returns. Only non-null fields
/// are asserted; <see cref="ExecutionId"/> accepts <c>"*"</c> for any non-empty
/// value.
/// </summary>
public sealed class TriggerExpectedResponseSpec
{
    public string? ExecutionId { get; set; }
    public int? Queued { get; set; }
    public bool? Deduplicated { get; set; }
}

public sealed class TriggerExpectationsSpec
{
    public int? DurationMaxMs { get; set; }
    public List<TriggerHttpExpectation> Http { get; set; } = [];
}

public sealed class TriggerHttpExpectation
{
    public string Method { get; set; } = "GET";
    public string Path { get; set; } = "/";
    public int? ExactCount { get; set; }
    public int? MinCount { get; set; }
    public int? MaxCount { get; set; }
    public Dictionary<string, string> Headers { get; set; } = new(StringComparer.OrdinalIgnoreCase);
    public object? BodyMatch { get; set; }

    /// <summary>
    /// Top-level request-body keys that must NOT be present. This is the
    /// assertion that pins "a producer must not fabricate defaults on the
    /// wire" — the contract #551 relied on and #553 extended to explicitly
    /// empty values. Asserted against the first matching request.
    /// </summary>
    public List<string> BodyAbsent { get; set; } = [];
}
