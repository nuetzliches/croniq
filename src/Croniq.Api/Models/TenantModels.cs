using System;
using System.ComponentModel.DataAnnotations;

namespace Croniq.Api.Models;

public sealed record UpsertTenantRequest(
    [property: Required] string Reference,
    [property: Required] string Name);

public sealed record TenantResponse(
    string TenantId,
    string Reference,
    string Name,
    bool IsActive,
    DateTimeOffset CreatedAtUtc);
