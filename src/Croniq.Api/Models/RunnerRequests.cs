using System;
using System.Collections.Generic;
using System.ComponentModel.DataAnnotations;

namespace Croniq.Api.Models;

public sealed record RunnerHeartbeatRequest(
    string? EnvironmentTag,
    [property: Required] string RunnerId,
    string? RunnerInstanceId = null,
    DateTimeOffset? SeenAtUtc = null,
    string? MetadataJson = null);

public sealed record RunnerStatusModel(
    [property: Required] string RunnerId,
    DateTimeOffset LastSeenAtUtc,
    DateTimeOffset ExpiresAtUtc,
    bool IsOnline,
    string? MetadataJson = null);

public sealed record RunnerListResponse(
    RunnerStatusModel[] Runners);

public sealed record RunnerJobRegistrationRequest(
    string? EnvironmentTag,
    [property: Required] string RunnerId,
    string? RunnerInstanceId,
    [property: Required] string JobKey,
    string? Description,
    IDictionary<string, string>? Metadata = null);
