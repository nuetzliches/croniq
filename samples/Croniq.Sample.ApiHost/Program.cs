using Croniq.Api;
using Croniq.Auth.Abstractions;
using Croniq.Auth.SqlServer;
using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Sample.Jobs;
using Croniq.Webhooks;
using Croniq.Data.SqlServer;
using Microsoft.AspNetCore.Identity;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Options;
using System.Linq;
using System.Reflection;

var builder = WebApplication.CreateBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();

builder.Services.AddCroniqWebhookServices(builder.Configuration);
builder.Services.AddCroniqWebhookRateLimiter();

var otelBuilder = builder.Services.AddCroniqApiObservability(
    builder.Configuration,
    builder.Logging);

var corsPolicyName = "CroniqSampleApiCors";
var allowedOrigins = builder.Configuration
    .GetSection("Croniq:Sample:Api:Cors:AllowedOrigins")
    .Get<string[]>() ?? Array.Empty<string>();

builder.Services.AddCors(options =>
{
    options.AddPolicy(corsPolicyName, policy =>
    {
        if (allowedOrigins.Length == 0)
        {
            if (builder.Environment.IsDevelopment())
            {
                policy.AllowAnyOrigin().AllowAnyHeader().AllowAnyMethod();
                return;
            }

            throw new InvalidOperationException("Croniq:Sample:Api:Cors:AllowedOrigins must be configured outside Development.");
        }

        policy
            .WithOrigins(allowedOrigins)
            .AllowAnyHeader()
            .AllowAnyMethod();
    });
});

// Persist execution logs locally for the sample host; production can swap to object storage or disable this.
builder.Logging.AddCroniqExecutionLogSink();
builder.Services.AddCroniqFileExecutionLogStore();
builder.Services.Configure<ExecutionLogRetentionOptions>(builder.Configuration.GetSection("Croniq:Logging:Execution:Retention"));
builder.Services.AddHostedService<ExecutionLogRetentionService>();

builder.Services.AddCroniqWebhookObservability(
    builder.Configuration,
    builder.Logging,
    builder: otelBuilder);

builder.Services.AddCroniqSampleJobs();
builder.Services.AddCroniqApiSchemas();

var app = builder.Build();

if (app.Environment.IsDevelopment())
{
    await using var scope = app.Services.CreateAsyncScope();
    var services = scope.ServiceProvider;

    var passwordAuthOptions = services.GetRequiredService<IOptions<PasswordAuthOptions>>().Value;
    if (passwordAuthOptions.Enabled)
    {
        var config = services.GetRequiredService<IConfiguration>();
        var authMode = (config["Croniq:Auth:Mode"] ?? string.Empty).Trim();
        if (!string.Equals(authMode, "SqlServer", StringComparison.OrdinalIgnoreCase))
        {
            app.Logger.LogInformation("Password auth seeding is enabled, but Croniq:Auth:Mode is '{AuthMode}'. Skipping password auth seeding.", authMode);
            goto after_password_seed;
        }

        var dbFactory = services.GetRequiredService<IDbContextFactory<SqlServerDbContext>>();
        await using (var db = await dbFactory.CreateDbContextAsync())
        {
            await db.Database.MigrateAsync();
        }

        var seedSection = config.GetSection("Croniq:Sample:Auth:Password");

        static string? Env(string name)
        {
            var value = Environment.GetEnvironmentVariable(name);
            return string.IsNullOrWhiteSpace(value) ? null : value.Trim();
        }

        static IReadOnlyCollection<string> ResolveSeedScopes(string? raw)
        {
            if (string.IsNullOrWhiteSpace(raw))
            {
                return new[]
                {
                    CroniqScopes.SchedulesWrite,
                    CroniqScopes.SchedulesDeadLetter,
                    CroniqScopes.JobsRead,
                    CroniqScopes.JobsWrite,
                    CroniqScopes.JobsTrigger,
                    CroniqScopes.ExecutionsRead,
                    CroniqScopes.WebhooksRead,
                    CroniqScopes.WebhooksWrite,
                    CroniqScopes.WebhooksRotate,
                    CroniqScopes.WebhooksDeadLetter,
                    CroniqScopes.ApiKeysManage,
                    CroniqScopes.TenantsAdmin,
                };
            }

            if (string.Equals(raw, "all", StringComparison.OrdinalIgnoreCase))
            {
                return typeof(CroniqScopes)
                    .GetFields(BindingFlags.Public | BindingFlags.Static)
                    .Where(field => field is { IsLiteral: true, IsInitOnly: false } && field.FieldType == typeof(string))
                    .Select(field => (string)field.GetRawConstantValue()!)
                    .Distinct(StringComparer.OrdinalIgnoreCase)
                    .OrderBy(scope => scope, StringComparer.OrdinalIgnoreCase)
                    .ToArray();
            }

            return raw
                .Split(new[] { ' ', '\t', '\r', '\n', ',' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .OrderBy(scope => scope, StringComparer.OrdinalIgnoreCase)
                .ToArray();
        }

        // Prefer CRONIQ_SEED_* env vars so Sample.ApiHost and devstack db-migrator seed the same tenant/user by default.
        // This avoids ending up with both 'default' and 'dev' tenants unless explicitly desired.
        var tenantReference = Env("CRONIQ_SEED_TENANT_REFERENCE") ?? seedSection["TenantReference"]?.Trim();
        if (string.IsNullOrWhiteSpace(tenantReference))
        {
            tenantReference = passwordAuthOptions.DefaultTenant?.Trim();
        }

        tenantReference ??= "dev";

        var tenantName = Env("CRONIQ_SEED_TENANT_NAME") ?? seedSection["TenantName"]?.Trim();
        tenantName ??= "Croniq Dev";

        var username = Env("CRONIQ_SEED_ADMIN_USERNAME") ?? seedSection["Username"]?.Trim();
        username ??= "admin";

        var password = Env("CRONIQ_SEED_ADMIN_PASSWORD") ?? seedSection["Password"];
        password ??= "admin";

        var scopes = ResolveSeedScopes(Env("CRONIQ_SEED_ADMIN_SCOPES"));

        var tenants = services.GetRequiredService<ITenantStore>();
        var tenant = await tenants.GetByReferenceAsync(tenantReference) ?? await tenants.CreateAsync(tenantReference, tenantName);

        var hasher = new PasswordHasher<object>();
        var passwordHash = hasher.HashPassword(new object(), password);

        var users = services.GetService<IPasswordUserStore>();
        if (users is null)
        {
            app.Logger.LogWarning("Password auth seeding requested, but IPasswordUserStore is not registered. Ensure Croniq auth mode is SqlServer and Croniq.Auth.SqlServer services are wired.");
            goto after_password_seed;
        }
        await users.UpsertAsync(new PasswordUserUpsertRequest(
            tenant.TenantId,
            username,
            passwordHash,
            scopes,
            IsActive: true,
            PasswordChangeRequired: true));
    }

after_password_seed:;
}

app.UseCroniqApiSwaggerUi(builder.Configuration);

app.UseCors(corsPolicyName);
app.UseCroniqApi();
app.MapCroniqSchedulerGrpc();
app.UseCroniqWebhooks(mapHealthEndpoints: false);

app.Run();
