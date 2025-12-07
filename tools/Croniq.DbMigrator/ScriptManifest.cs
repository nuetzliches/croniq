namespace Croniq.DbMigrator;

internal static class ScriptManifest
{
    public static readonly string[] SqlScripts =
    [
        "predeploy.sql",
        "core/types.sql",
        "core/procs.health.sql",
        "core-internal/types.sql",
        "core-internal/procs.errors.sql",
        "core-internal/procs.guards.sql",
        "croniq/types.sql",
        "croniq/functions.sql",
        "croniq-internal/types.sql",
        "croniq-internal/procs.errors.sql",
        "croniq-internal/procs.guards.sql",
        "auth/types.sql",
        "auth/tables.sql",
        "auth/procs.keys.sql",
        "croniq/tables.sql",
        "croniq/procs.instances.sql",
        "croniq/procs.jobs.sql",
        "croniq/procs.leases.sql",
        "croniq/procs.deadletter.sql",
        "seed.dev.sql"
    ];
}
