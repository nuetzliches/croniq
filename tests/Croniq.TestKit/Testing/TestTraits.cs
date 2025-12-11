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
    }
}
