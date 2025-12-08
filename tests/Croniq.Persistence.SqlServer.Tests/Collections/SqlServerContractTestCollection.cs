using Croniq.TestKit.SqlServer;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests.Collections;

[CollectionDefinition(Name, DisableParallelization = true)]
public sealed class SqlServerContractTestCollection : ICollectionFixture<SqlServerContainerFixture>
{
    public const string Name = "SqlServerContractTests";
}
