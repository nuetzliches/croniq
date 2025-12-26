using System;
using System.ComponentModel.DataAnnotations;

namespace Croniq.Api.Models;

public sealed record RunnerHeartbeatRequest(
    string? EnvironmentTag,
    [property: Required] string RunnerId,
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
