namespace Croniq.Api.Models;

public sealed record HealthStatusResponse(
    string Status);

public sealed record PersistenceHealthResponse(
    string Status,
    string Provider,
    string? Note,
    string? Db);
