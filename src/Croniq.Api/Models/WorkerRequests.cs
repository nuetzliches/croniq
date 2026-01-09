using System;
using System.ComponentModel.DataAnnotations;

namespace Croniq.Api.Models;

public sealed record WorkerHeartbeatRequest(
    string? EnvironmentTag,
    [property: Required] string InstanceId,
    DateTimeOffset? SeenAtUtc = null,
    string? MetadataJson = null);

public sealed record WorkerStatusModel(
    [property: Required] string InstanceId,
    DateTimeOffset LastSeenAtUtc,
    DateTimeOffset ExpiresAtUtc,
    bool IsOnline,
    string? MetadataJson = null);

public sealed record WorkerListResponse(
    WorkerStatusModel[] Workers);
