namespace Croniq.TestKit.Testing;

/// <summary>
/// Shared trait keys/values for selectively including or excluding Croniq test components.
/// </summary>
public static class TestTraits
{
    /// <summary>
    /// Trait key for component-level filtering (e.g., Component=Persistence.SqlServer.Jobs).
    /// </summary>
    public const string Component = "Component";

    public static class Components
    {
        public const string SqlPersistenceJobs = "Persistence.SqlServer.Jobs";
        public const string SqlPersistenceWebhooks = "Persistence.SqlServer.Webhooks";
        public const string SqlPersistenceDeadLetters = "Persistence.SqlServer.WebhookDeadLetters";
        public const string SqlPersistenceChangefeed = "Persistence.SqlServer.WebhookChangefeed";
        public const string SqlPersistenceWorkers = "Persistence.SqlServer.Workers";
        public const string SqlPersistenceRunners = "Persistence.SqlServer.Runners";
        public const string SqlPersistenceWorkItems = "Persistence.SqlServer.WorkItems";

        public const string PostgresPersistenceJobs = "Persistence.Postgres.Jobs";
        public const string PostgresPersistenceWebhooks = "Persistence.Postgres.Webhooks";
        public const string PostgresPersistenceDeadLetters = "Persistence.Postgres.WebhookDeadLetters";
        public const string PostgresPersistenceChangefeed = "Persistence.Postgres.WebhookChangefeed";
        public const string PostgresPersistenceWorkers = "Persistence.Postgres.Workers";
        public const string PostgresPersistenceRunners = "Persistence.Postgres.Runners";
        public const string PostgresPersistenceWorkItems = "Persistence.Postgres.WorkItems";
    }
}
