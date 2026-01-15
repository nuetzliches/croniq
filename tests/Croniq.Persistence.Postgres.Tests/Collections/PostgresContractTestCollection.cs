using Croniq.TestKit.Postgres;
using Xunit;

namespace Croniq.Persistence.Postgres.Tests.Collections;

[CollectionDefinition(Name, DisableParallelization = true)]
public sealed class PostgresContractTestCollection : ICollectionFixture<PostgresContainerFixture>
{
    public const string Name = "PostgresContractTests";
}


