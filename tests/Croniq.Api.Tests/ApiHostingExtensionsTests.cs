using Croniq.Api;
using Microsoft.AspNetCore.Builder;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Options;
using Shouldly;
using Swashbuckle.AspNetCore.SwaggerGen;
using Xunit;

namespace Croniq.Api.Tests;

public class ApiHostingExtensionsTests
{
    [Fact]
    public void UseCroniqApiSwaggerUi_ConfiguresSwaggerAndReflection()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            EnvironmentName = Environments.Development
        });

        builder.Services.AddLogging();
        builder.Services.AddGrpc();
        builder.Services.AddCroniqApiSchemas();

        var app = builder.Build();

        var swaggerOptions = app.Services.GetRequiredService<IOptions<SwaggerGenOptions>>().Value;
        swaggerOptions.ShouldNotBeNull();

        var returned = app.UseCroniqApiSwaggerUi(builder.Configuration);

        returned.ShouldBeSameAs(app);
    }
}
