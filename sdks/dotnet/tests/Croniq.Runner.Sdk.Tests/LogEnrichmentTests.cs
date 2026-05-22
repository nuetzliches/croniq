using Croniq.Runner.Sdk.Internal;
using Croniq.Runner.Sdk.Protocol;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

public class LogEnrichmentTests
{
    [Fact]
    public void Enrich_AutoInjectsJobAndRunnerFields()
    {
        var enrichment = new LogEnrichment("billing:invoice", "runner-1", ["env=prod"]);
        var source = new WorkEvent { Message = "hello" };

        var enriched = enrichment.Enrich(source);

        enriched.Fields.ShouldNotBeNull();
        enriched.Fields!["job_key"].ShouldBe("billing:invoice");
        enriched.Fields["runner_id"].ShouldBe("runner-1");
        enriched.Fields["runner_tags"].ShouldBe("[\"env=prod\"]");
    }

    [Fact]
    public void Enrich_DoesNotOverwriteExplicitCallerFields()
    {
        var enrichment = new LogEnrichment("billing:invoice", "runner-1", []);
        var source = new WorkEvent
        {
            Message = "hello",
            Fields = new Dictionary<string, string> { ["job_key"] = "explicit-override" },
        };

        var enriched = enrichment.Enrich(source);

        enriched.Fields!["job_key"].ShouldBe("explicit-override");
    }

    [Fact]
    public void Enrich_OmitsRunnerTagsKeyWhenNoTagsConfigured()
    {
        var enrichment = new LogEnrichment("billing:invoice", "runner-1", []);
        var source = new WorkEvent { Message = "hello" };

        var enriched = enrichment.Enrich(source);

        enriched.Fields!.Keys.ShouldNotContain("runner_tags");
    }
}
