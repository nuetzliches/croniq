using System;
using System.Collections.Generic;
using Croniq.Auth.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public class AuthContractsTests
{
    [Fact]
    public void UserDescriptor_AssignsValues()
    {
        var roles = new List<string> { "admin" };
        var descriptor = new UserDescriptor(
            "user-1",
            "tenant-1",
            "subject-1",
            "issuer-1",
            "user@example.com",
            "User One",
            roles,
            true);

        descriptor.UserId.ShouldBe("user-1");
        descriptor.TenantId.ShouldBe("tenant-1");
        descriptor.Subject.ShouldBe("subject-1");
        descriptor.Issuer.ShouldBe("issuer-1");
        descriptor.Email.ShouldBe("user@example.com");
        descriptor.DisplayName.ShouldBe("User One");
        descriptor.Roles.ShouldBe(roles);
        descriptor.IsActive.ShouldBeTrue();
    }
}
