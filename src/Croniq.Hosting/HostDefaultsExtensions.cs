using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Server.Kestrel.Core;
using Microsoft.Extensions.Configuration;

namespace Croniq.Hosting;

public static class HostDefaultsExtensions
{
    public static WebApplicationBuilder AddCroniqHostDefaults(this WebApplicationBuilder builder)
    {
        if (builder is null)
        {
            throw new ArgumentNullException(nameof(builder));
        }

        builder.Configuration
            .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
            .AddEnvironmentVariables();

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.ConfigureEndpointDefaults(endpoint =>
            {
                endpoint.Protocols = HttpProtocols.Http1AndHttp2;
            });
        });

        return builder;
    }
}
