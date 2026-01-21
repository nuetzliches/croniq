using System.Globalization;
using Microsoft.Data.SqlClient;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Diagnostics.HealthChecks;

var repoRoot = FindRepoRoot(Directory.GetCurrentDirectory()) ?? Directory.GetCurrentDirectory();
var envValues = LoadEnvFile(Path.Combine(repoRoot, ".env"));

string? GetEnvValue(string name)
{
    var direct = Environment.GetEnvironmentVariable(name);
    if (!string.IsNullOrWhiteSpace(direct))
    {
        return direct.Trim();
    }

    if (envValues.TryGetValue(name, out var value) && !string.IsNullOrWhiteSpace(value))
    {
        return value.Trim();
    }

    return null;
}

string GetEnvValueOrDefault(string name, string fallback) => GetEnvValue(name) ?? fallback;

int GetInt(string name, int fallback)
{
    var raw = GetEnvValue(name);
    return int.TryParse(raw, NumberStyles.Integer, CultureInfo.InvariantCulture, out var value)
        ? value
        : fallback;
}

EnsureDashboardEnvironment();

var builderOptions = new DistributedApplicationOptions
{
    Args = args,
    AllowUnsecuredTransport = true
};
var builder = DistributedApplication.CreateBuilder(builderOptions);

var dotnetEnvironment = GetEnvValueOrDefault("CRONIQ_DOTNET_ENVIRONMENT", "Development");
var tenantMode = GetEnvValueOrDefault("CRONIQ_CORE_TENANT_MODE", "Single");
var tenantId = GetEnvValueOrDefault("CRONIQ_CORE_TENANT_ID", "default");
var tenantName = GetEnvValueOrDefault("CRONIQ_CORE_TENANT_NAME", "Default");
var environmentTag = GetEnvValueOrDefault("CRONIQ_ENVIRONMENT", "dev");
var apiInstanceId = GetEnvValueOrDefault("CRONIQ_API_INSTANCE_ID", "api-dev");
var workerInstanceId = GetEnvValueOrDefault("CRONIQ_WORKER_INSTANCE_ID", "worker-dev");
var apiRequestsPerMinute = GetEnvValueOrDefault("CRONIQ_API_REQUESTS_PER_MINUTE", "240");
var apiKey = GetEnvValueOrDefault("CRONIQ_API_KEY", "smoke-key");
var authMode = GetEnvValueOrDefault("CRONIQ_AUTH_MODE", "InMemory");
var apiPort = GetInt("CRONIQ_API_HTTP_PORT", GetInt("CRONIQ_API_INTERNAL_PORT", 5080));
var uiPort = GetInt("CRONIQ_UI_HTTP_PORT", 5081);
var sqlHostPort = GetInt("CRONIQ_SQL_HOST_PORT", 11433);
var sqlDatabase = GetEnvValueOrDefault("CRONIQ_SQL_DATABASE", "CroniqDev");
var sqlPassword = GetEnvValueOrDefault("CRONIQ_SQL_PASSWORD", "CroniqSqlP@ssw0rd!");
var sqlHost = GetEnvValue("CRONIQ_SQL_HOST");
var dmzHttpPort = GetInt("CRONIQ_SAMPLE_DMZ_HTTP_PORT", 5000);
var dmzGrpcPort = GetInt("CRONIQ_SAMPLE_DMZ_GRPC_PORT", 5001);
var dmzSqlDatabase = GetEnvValueOrDefault("CRONIQ_SAMPLE_DMZ_SQL_DATABASE", "CroniqDmz");
var dmzAuthMode = GetEnvValueOrDefault("CRONIQ_SAMPLE_DMZ_AUTH_MODE", "InMemory");
var dmzInstanceId = GetEnvValueOrDefault("CRONIQ_SAMPLE_DMZ_INSTANCE_ID", "dmz-dev");
var dmzBaseUrl = GetEnvValueOrDefault("CRONIQ_SAMPLE_DMZ_BASEURL", $"https://localhost:{dmzGrpcPort}");
var dmzApiKey = GetEnvValueOrDefault("CRONIQ_SAMPLE_DMZ_API_KEY", "dmz-sample-key");
var caddyDomain = GetEnvValueOrDefault("CRONIQ_CADDY_DOMAIN", "croniq.local");
var caddyUpstreamHost = GetEnvValueOrDefault("CRONIQ_CADDY_UPSTREAM_HOST", "host.docker.internal");
var caddyHttpPort = GetInt("CRONIQ_CADDY_HTTP_PORT", 80);
var caddyHttpsPort = GetInt("CRONIQ_CADDY_HTTPS_PORT", 443);
var caddyEnabled = !IsFalse(GetEnvValue("CRONIQ_DEVSTACK_CADDY"));
var caddyHttpsPortSuffix = caddyHttpsPort == 443
    ? string.Empty
    : string.Concat(":", caddyHttpsPort.ToString(CultureInfo.InvariantCulture));
var caddyApiUrl = caddyEnabled
    ? string.Concat("https://api.", caddyDomain, caddyHttpsPortSuffix)
    : null;
var caddyUiUrl = caddyEnabled
    ? string.Concat("https://ui.", caddyDomain, caddyHttpsPortSuffix)
    : null;
var caddyDmzUrl = caddyEnabled
    ? string.Concat("https://dmz.", caddyDomain, caddyHttpsPortSuffix)
    : null;
var uiApiBaseUrl = GetEnvValue("CRONIQ_UI_API_BASEURL");
var uiSwaggerUiUrl = GetEnvValue("CRONIQ_UI_SWAGGER_UI_URL") ?? GetEnvValue("CRONIQ_UI_SWAGGER_URL");
var uiDefaultTenantId = GetEnvValue("CRONIQ_UI_DEFAULT_TENANT_ID");
var uiActivityStreamMode = GetEnvValue("CRONIQ_UI_WEBHOOKS_ACTIVITY_STREAM_MODE");
var uiActivityGrpcBaseUrl = GetEnvValue("CRONIQ_UI_WEBHOOKS_ACTIVITY_GRPC_BASEURL");
var uiActivitySseBaseUrl = GetEnvValue("CRONIQ_UI_WEBHOOKS_ACTIVITY_SSE_BASEURL");
var uiEnabled = !IsFalse(GetEnvValue("CRONIQ_DEVSTACK_UI"));
var otlpEndpoint = GetEnvValue("CRONIQ_OBS_OTLP_ENDPOINT");
var otlpProtocol = GetEnvValue("CRONIQ_OBS_OTLP_PROTOCOL");
var otlpGrpcPort = GetInt("CRONIQ_OTLP_GRPC_PORT", 4317);
var otlpHttpPort = GetInt("CRONIQ_OTLP_HTTP_PORT", 4318);
var otelPromPort = GetInt("CRONIQ_OTEL_PROM_PORT", 8889);
var prometheusPort = GetInt("CRONIQ_PROMETHEUS_PORT", 9090);
var grafanaPort = GetInt("CRONIQ_GRAFANA_PORT", 5610);
var tempoHttpPort = GetInt("CRONIQ_TEMPO_HTTP_PORT", 3200);
var lokiHttpPort = GetInt("CRONIQ_LOKI_HTTP_PORT", 3100);
var grafanaUser = GetEnvValueOrDefault("CRONIQ_GRAFANA_USER", "admin");
var grafanaPassword = GetEnvValueOrDefault("CRONIQ_GRAFANA_PASSWORD", "admin");
var obsEnabled = IsObsEnabled(
    args,
    GetEnvValue("CRONIQ_DEVSTACK_PROFILES"),
    GetEnvValue("CRONIQ_DEVSTACK_OBS"));

if (obsEnabled)
{
    if (string.IsNullOrWhiteSpace(otlpEndpoint) || IsContainerCollectorEndpoint(otlpEndpoint))
    {
        otlpEndpoint = string.Concat("http://localhost:", otlpGrpcPort.ToString(CultureInfo.InvariantCulture));
    }

    if (string.IsNullOrWhiteSpace(otlpProtocol))
    {
        otlpProtocol = "grpc";
    }
}

if (caddyEnabled)
{
    if (string.IsNullOrWhiteSpace(uiApiBaseUrl))
    {
        uiApiBaseUrl = string.Concat(
            "https://api.",
            caddyDomain,
            caddyHttpsPortSuffix);
    }
}

if (!string.IsNullOrWhiteSpace(Environment.GetEnvironmentVariable("CI"))
    || !string.IsNullOrWhiteSpace(Environment.GetEnvironmentVariable("GITHUB_ACTIONS")))
{
    uiEnabled = false;
}

var forwardedHeadersEnabled = GetEnvValueOrDefault("CRONIQ_API_FORWARDED_HEADERS_ENABLED", "false");
var forwardedHeadersForwardLimit = GetEnvValueOrDefault("CRONIQ_API_FORWARDED_HEADERS_FORWARD_LIMIT", "1");
var forwardedHeadersKnownNetwork0 = GetEnvValue("CRONIQ_API_FORWARDED_HEADERS_KNOWN_NETWORKS_0");
var forwardedHeadersKnownProxy0 = GetEnvValue("CRONIQ_API_FORWARDED_HEADERS_KNOWN_PROXIES_0");
var dbProvider = GetEnvValue("CRONIQ_DB_PROVIDER");
var postgresConnection = GetEnvValue("CRONIQ_POSTGRES_CONNECTION");
var usePostgres = string.Equals(dbProvider, "Postgres", StringComparison.OrdinalIgnoreCase)
    || (string.IsNullOrWhiteSpace(dbProvider) && !string.IsNullOrWhiteSpace(postgresConnection));
var dmzEnabled = true;
var needsSqlServer = !usePostgres || dmzEnabled;

string ResolveSqlHost()
{
    if (string.IsNullOrWhiteSpace(sqlHost) || string.Equals(sqlHost, "mssql-22", StringComparison.OrdinalIgnoreCase))
    {
        return "127.0.0.1";
    }

    return sqlHost;
}

var sqlConnection = GetEnvValue("CRONIQ_SQL_CONNECTION");
if (!usePostgres && string.IsNullOrWhiteSpace(sqlConnection))
{
    sqlConnection = $"Server={ResolveSqlHost()},{sqlHostPort};Database={sqlDatabase};User Id=sa;Password={sqlPassword};Encrypt=False;TrustServerCertificate=True;";
}

var logsPath = Path.Combine(repoRoot, "logs");
var apiUrls = string.Concat("http://0.0.0.0:", apiPort.ToString(CultureInfo.InvariantCulture));
var dmzUrls = string.Concat(
    "https://0.0.0.0:",
    dmzGrpcPort.ToString(CultureInfo.InvariantCulture),
    ";http://0.0.0.0:",
    dmzHttpPort.ToString(CultureInfo.InvariantCulture));
var dmzSqlConnection = $"Server={ResolveSqlHost()},{sqlHostPort};Database={dmzSqlDatabase};User Id=sa;Password={sqlPassword};Encrypt=False;TrustServerCertificate=True;";
const string sqlReadyHealthCheckName = "croniq-sql-ready";
var sqlHealthConnectionString = sqlConnection ?? dmzSqlConnection;
if (needsSqlServer && !string.IsNullOrWhiteSpace(sqlHealthConnectionString))
{
    builder.Services
        .AddHealthChecks()
        .AddCheck(sqlReadyHealthCheckName, new SqlServerReadyHealthCheck(sqlHealthConnectionString));
}

IResourceBuilder<ContainerResource>? sqlServer = null;
if (needsSqlServer)
{
    sqlServer = builder.AddContainer("mssql-22", "mcr.microsoft.com/mssql/server", "2022-latest")
        .WithEnvironment("ACCEPT_EULA", "Y")
        .WithEnvironment("MSSQL_SA_PASSWORD", sqlPassword)
        .WithEnvironment("MSSQL_PID", "Developer")
        .WithEndpoint(
            targetPort: 1433,
            port: sqlHostPort,
            scheme: "tcp",
            name: "sql",
            env: null,
            isExternal: true,
            isProxied: false)
        .WithVolume("croniq-mssql-data", "/var/opt/mssql", isReadOnly: false);

    if (!string.IsNullOrWhiteSpace(sqlHealthConnectionString))
    {
        sqlServer.WithHealthCheck(sqlReadyHealthCheckName);
    }
}

var migrator = builder.AddProject(
        "croniq-db-migrator",
        Path.Combine(repoRoot, "tools", "Croniq.DbMigrator", "Croniq.DbMigrator.csproj"))
    .WithEnvironment("DOTNET_ENVIRONMENT", dotnetEnvironment)
    .WithEnvironment("CRONIQ_SEED_ADMIN", GetEnvValueOrDefault("CRONIQ_SEED_ADMIN", "true"))
    .WithEnvironment("CRONIQ_SEED_TENANT_ID", GetEnvValueOrDefault("CRONIQ_SEED_TENANT_ID", string.Empty))
    .WithEnvironment("CRONIQ_SEED_TENANT_NAME", GetEnvValueOrDefault("CRONIQ_SEED_TENANT_NAME", string.Empty))
    .WithEnvironment("CRONIQ_SEED_TENANT_REFERENCE", GetEnvValueOrDefault("CRONIQ_SEED_TENANT_REFERENCE", string.Empty))
    .WithEnvironment("CRONIQ_CORE_TENANT_ID", tenantId)
    .WithEnvironment("CRONIQ_CORE_TENANT_NAME", tenantName)
    .WithEnvironment("CRONIQ_SEED_ADMIN_USERNAME", GetEnvValueOrDefault("CRONIQ_SEED_ADMIN_USERNAME", "admin"))
    .WithEnvironment("CRONIQ_SEED_ADMIN_PASSWORD", GetEnvValueOrDefault("CRONIQ_SEED_ADMIN_PASSWORD", "admin"))
    .WithEnvironment("CRONIQ_SEED_ADMIN_PASSWORD_CHANGE_REQUIRED", GetEnvValueOrDefault("CRONIQ_SEED_ADMIN_PASSWORD_CHANGE_REQUIRED", "true"))
    .WithEnvironment("CRONIQ_SEED_ADMIN_SCOPES", GetEnvValueOrDefault("CRONIQ_SEED_ADMIN_SCOPES", "all"));

if (!string.IsNullOrWhiteSpace(dbProvider))
{
    migrator.WithEnvironment("CRONIQ_DB_PROVIDER", dbProvider);
}

if (!string.IsNullOrWhiteSpace(sqlConnection))
{
    migrator.WithEnvironment("CRONIQ_SQL_CONNECTION", sqlConnection);
}

if (!string.IsNullOrWhiteSpace(postgresConnection))
{
    migrator.WithEnvironment("CRONIQ_POSTGRES_CONNECTION", postgresConnection);
}

if (sqlServer is not null)
{
    migrator.WaitFor(sqlServer);
}

var dmzMigrator = builder.AddProject(
        "croniq-db-migrator-dmz",
        Path.Combine(repoRoot, "tools", "Croniq.DbMigrator", "Croniq.DbMigrator.csproj"))
    .WithEnvironment("DOTNET_ENVIRONMENT", dotnetEnvironment)
    .WithEnvironment("CRONIQ_DB_PROVIDER", "SqlServer")
    .WithEnvironment("CRONIQ_SQL_CONNECTION", dmzSqlConnection)
    .WithEnvironment("CRONIQ_SEED_ADMIN", GetEnvValueOrDefault("CRONIQ_SEED_ADMIN", "true"))
    .WithEnvironment("CRONIQ_SEED_TENANT_ID", GetEnvValueOrDefault("CRONIQ_SEED_TENANT_ID", string.Empty))
    .WithEnvironment("CRONIQ_SEED_TENANT_NAME", GetEnvValueOrDefault("CRONIQ_SEED_TENANT_NAME", string.Empty))
    .WithEnvironment("CRONIQ_SEED_TENANT_REFERENCE", GetEnvValueOrDefault("CRONIQ_SEED_TENANT_REFERENCE", string.Empty))
    .WithEnvironment("CRONIQ_CORE_TENANT_ID", tenantId)
    .WithEnvironment("CRONIQ_CORE_TENANT_NAME", tenantName)
    .WithEnvironment("CRONIQ_SEED_ADMIN_USERNAME", GetEnvValueOrDefault("CRONIQ_SEED_ADMIN_USERNAME", "admin"))
    .WithEnvironment("CRONIQ_SEED_ADMIN_PASSWORD", GetEnvValueOrDefault("CRONIQ_SEED_ADMIN_PASSWORD", "admin"))
    .WithEnvironment("CRONIQ_SEED_ADMIN_PASSWORD_CHANGE_REQUIRED", GetEnvValueOrDefault("CRONIQ_SEED_ADMIN_PASSWORD_CHANGE_REQUIRED", "true"))
    .WithEnvironment("CRONIQ_SEED_ADMIN_SCOPES", GetEnvValueOrDefault("CRONIQ_SEED_ADMIN_SCOPES", "all"));

if (sqlServer is not null)
{
    dmzMigrator.WaitFor(sqlServer);
}

if (obsEnabled)
{
    var otelCollectorConfig = Path.Combine(repoRoot, "infra", "docker", "observability", "otel-collector-config.yaml");
    var prometheusConfig = Path.Combine(repoRoot, "infra", "docker", "observability", "prometheus.yaml");
    var prometheusRules = Path.Combine(repoRoot, "infra", "monitoring", "rules");
    var tempoConfig = Path.Combine(repoRoot, "infra", "docker", "observability", "tempo.yaml");
    var lokiConfig = Path.Combine(repoRoot, "infra", "docker", "observability", "loki-config.yaml");
    var grafanaDatasources = Path.Combine(repoRoot, "infra", "docker", "observability", "grafana", "datasources");
    var grafanaProvisioning = Path.Combine(repoRoot, "infra", "docker", "observability", "grafana", "provisioning", "dashboards");
    var grafanaDashboards = Path.Combine(repoRoot, "infra", "docker", "observability", "grafana", "dashboards");

    var tempo = builder.AddContainer("tempo", "grafana/tempo", "2.4.1")
        .WithArgs("-config.file=/etc/tempo.yaml")
        .WithBindMount(tempoConfig, "/etc/tempo.yaml", isReadOnly: true)
        .WithVolume("tempo-data", "/tmp/tempo", isReadOnly: false)
        .WithEndpoint(
            targetPort: 3200,
            port: tempoHttpPort,
            scheme: "http",
            name: "tempo",
            env: null,
            isExternal: true,
            isProxied: false);

    var loki = builder.AddContainer("loki", "grafana/loki", "3.1.1")
        .WithArgs("-config.file=/etc/loki/local-config.yaml")
        .WithBindMount(lokiConfig, "/etc/loki/local-config.yaml", isReadOnly: true)
        .WithVolume("loki-data", "/loki", isReadOnly: false)
        .WithEndpoint(
            targetPort: 3100,
            port: lokiHttpPort,
            scheme: "http",
            name: "loki",
            env: null,
            isExternal: true,
            isProxied: false);

    builder.AddContainer("otel-collector", "otel/opentelemetry-collector-contrib", "0.102.1")
        .WithArgs("--config=/etc/otel-collector-config.yaml")
        .WithBindMount(otelCollectorConfig, "/etc/otel-collector-config.yaml", isReadOnly: true)
        .WithEndpoint(
            targetPort: 4317,
            port: otlpGrpcPort,
            scheme: "http",
            name: "otlp-grpc",
            env: null,
            isExternal: true,
            isProxied: false)
        .WithEndpoint(
            targetPort: 4318,
            port: otlpHttpPort,
            scheme: "http",
            name: "otlp-http",
            env: null,
            isExternal: true,
            isProxied: false)
        .WithEndpoint(
            targetPort: 8889,
            port: otelPromPort,
            scheme: "http",
            name: "otel-prom",
            env: null,
            isExternal: true,
            isProxied: false)
        .WaitFor(tempo)
        .WaitFor(loki);

    var prometheus = builder.AddContainer("prometheus", "prom/prometheus", "v2.54.1")
        .WithArgs("--config.file=/etc/prometheus/prometheus.yml")
        .WithBindMount(prometheusConfig, "/etc/prometheus/prometheus.yml", isReadOnly: true)
        .WithBindMount(prometheusRules, "/etc/prometheus/rules", isReadOnly: true)
        .WithVolume("prom-data", "/prometheus", isReadOnly: false)
        .WithEndpoint(
            targetPort: 9090,
            port: prometheusPort,
            scheme: "http",
            name: "prometheus",
            env: null,
            isExternal: true,
            isProxied: false);

    builder.AddContainer("grafana", "grafana/grafana", "11.2.0")
        .WithEnvironment("GF_SECURITY_ADMIN_PASSWORD", grafanaPassword)
        .WithEnvironment("GF_SECURITY_ADMIN_USER", grafanaUser)
        .WithEnvironment("GF_SECURITY_ALLOW_EMBEDDING", "true")
        .WithEnvironment("GF_PATHS_PROVISIONING", "/etc/grafana/provisioning")
        .WithBindMount(grafanaDatasources, "/etc/grafana/provisioning/datasources", isReadOnly: true)
        .WithBindMount(grafanaProvisioning, "/etc/grafana/provisioning/dashboards", isReadOnly: true)
        .WithBindMount(grafanaDashboards, "/var/lib/grafana/dashboards", isReadOnly: true)
        .WithVolume("grafana-data", "/var/lib/grafana", isReadOnly: false)
        .WithEndpoint(
            targetPort: 3000,
            port: grafanaPort,
            scheme: "http",
            name: "grafana",
            env: null,
            isExternal: true,
            isProxied: false)
        .WaitFor(prometheus)
        .WaitFor(tempo)
        .WaitFor(loki);
}

if (caddyEnabled)
{
    var caddyFile = Path.Combine(repoRoot, "infra", "docker", "caddy", "Caddyfile");
    builder.AddContainer("caddy", "caddy", "2.8.4")
        .WithBindMount(caddyFile, "/etc/caddy/Caddyfile", isReadOnly: true)
        .WithVolume("caddy-data", "/data", isReadOnly: false)
        .WithVolume("caddy-config", "/config", isReadOnly: false)
        .WithEnvironment("CRONIQ_CADDY_DOMAIN", caddyDomain)
        .WithEnvironment("CRONIQ_CADDY_UPSTREAM_HOST", caddyUpstreamHost)
        .WithEnvironment("CRONIQ_CADDY_HTTP_PORT", caddyHttpPort.ToString(CultureInfo.InvariantCulture))
        .WithEnvironment("CRONIQ_CADDY_HTTPS_PORT", caddyHttpsPort.ToString(CultureInfo.InvariantCulture))
        .WithEnvironment("CRONIQ_API_HTTP_PORT", apiPort.ToString(CultureInfo.InvariantCulture))
        .WithEnvironment("CRONIQ_SAMPLE_DMZ_HTTP_PORT", dmzHttpPort.ToString(CultureInfo.InvariantCulture))
        .WithEnvironment("CRONIQ_SAMPLE_DMZ_GRPC_PORT", dmzGrpcPort.ToString(CultureInfo.InvariantCulture))
        .WithEnvironment("CRONIQ_UI_HTTP_PORT", uiPort.ToString(CultureInfo.InvariantCulture))
        .WithEndpoint(
            targetPort: caddyHttpPort,
            port: caddyHttpPort,
            scheme: "http",
            name: "caddy-http",
            env: null,
            isExternal: true,
            isProxied: false)
        .WithEndpoint(
            targetPort: caddyHttpsPort,
            port: caddyHttpsPort,
            scheme: "https",
            name: "caddy-https",
            env: null,
            isExternal: true,
            isProxied: false);
}

var api = builder.AddProject(
        "croniq-api",
        Path.Combine(repoRoot, "samples", "Croniq.Sample.ApiHost", "Croniq.Sample.ApiHost.csproj"),
        options =>
        {
            options.ExcludeLaunchProfile = true;
            options.ExcludeKestrelEndpoints = true;
        })
    .WithHttpEndpoint(targetPort: apiPort, port: apiPort, name: "http", env: null, isProxied: false)
    .WithEnvironment("DOTNET_ENVIRONMENT", dotnetEnvironment)
    .WithEnvironment("ASPNETCORE_ENVIRONMENT", dotnetEnvironment)
    .WithEnvironment("ASPNETCORE_URLS", apiUrls)
    .WithEnvironment("Croniq__Core__TenantMode", tenantMode)
    .WithEnvironment("Croniq__Core__TenantId", tenantId)
    .WithEnvironment("Croniq__Core__EnvironmentTag", environmentTag)
    .WithEnvironment("Croniq__Core__InstanceId", apiInstanceId)
    .WithEnvironment("Croniq__Api__RequestsPerMinute", apiRequestsPerMinute)
    .WithEnvironment("Croniq__Api__ForwardedHeaders__Enabled", forwardedHeadersEnabled)
    .WithEnvironment("Croniq__Api__ForwardedHeaders__ForwardLimit", forwardedHeadersForwardLimit)
    .WithEnvironment("Croniq__Auth__Mode", authMode)
    .WithEnvironment("Croniq__Auth__InMemory__ApiKey", apiKey)
    .WithEnvironment("Croniq__Auth__InMemory__TenantId", tenantId)
    .WithEnvironment("Croniq__Auth__InMemory__EnvironmentTag", environmentTag)
    .WithEnvironment("Croniq__Webhooks__Remote__BaseUrl", dmzBaseUrl)
    .WithEnvironment("Croniq__Webhooks__Remote__ApiKey", dmzApiKey)
    .WithEnvironment("Croniq__Webhooks__Remote__AllowInvalidServerCertificate", "true")
    .WithEnvironment("Croniq__Webhooks__Remote__EnableRelay", "false")
    .WithEnvironment("Croniq__Logging__Execution__BasePath", logsPath)
    .WaitForCompletion(migrator, exitCode: 0);

if (!string.IsNullOrWhiteSpace(caddyApiUrl))
{
    api.WithUrlForEndpoint("http", url =>
    {
        url.Url = caddyApiUrl;
        // url.DisplayText = caddyApiUrl;
    });
}

if (!string.IsNullOrWhiteSpace(caddyUiUrl))
{
    api.WithEnvironment("CroniqSample__Api__Cors__AllowedOrigins__2", caddyUiUrl);
}

if (usePostgres)
{
    api.WithEnvironment("Croniq__Persistence__Mode", "Postgres");
    if (!string.IsNullOrWhiteSpace(postgresConnection))
    {
        api.WithEnvironment("Croniq__Postgres__ConnectionString", postgresConnection);
    }
}
else if (!string.IsNullOrWhiteSpace(sqlConnection))
{
    api.WithEnvironment("Croniq__Persistence__Mode", "SqlServer")
        .WithEnvironment("Croniq__SqlServer__ConnectionString", sqlConnection);
}

if (!string.IsNullOrWhiteSpace(forwardedHeadersKnownNetwork0))
{
    api.WithEnvironment("Croniq__Api__ForwardedHeaders__KnownNetworks__0", forwardedHeadersKnownNetwork0);
}

if (!string.IsNullOrWhiteSpace(forwardedHeadersKnownProxy0))
{
    api.WithEnvironment("Croniq__Api__ForwardedHeaders__KnownProxies__0", forwardedHeadersKnownProxy0);
}

if (!string.IsNullOrWhiteSpace(otlpEndpoint))
{
    api.WithEnvironment("Croniq__Observability__OtlpEndpoint", otlpEndpoint);
}

if (!string.IsNullOrWhiteSpace(otlpProtocol))
{
    api.WithEnvironment("Croniq__Observability__OtlpProtocol", otlpProtocol);
}

if (uiEnabled)
{
    var uiPath = Path.Combine(repoRoot, "src", "Croniq.Ui");
    var npmCommand = OperatingSystem.IsWindows() ? "npm.cmd" : "npm";
    var uiArgs = new[]
    {
        "run",
        "start",
        "--",
        "--port",
        uiPort.ToString(CultureInfo.InvariantCulture),
        "--host",
        "0.0.0.0"
    };

    var ui = builder.AddExecutable("croniq-ui", npmCommand, uiPath, uiArgs)
        .WithHttpEndpoint(targetPort: uiPort, port: uiPort, isProxied: false)
        .WithEnvironment("CRONIQ_UI_HTTP_PORT", uiPort.ToString(CultureInfo.InvariantCulture))
        .WaitFor(api);

    if (!string.IsNullOrWhiteSpace(caddyUiUrl))
    {
        ui.WithUrlForEndpoint("http", url =>
        {
            url.Url = caddyUiUrl;
            // url.DisplayText = caddyUiUrl;
        });
    }

    if (!string.IsNullOrWhiteSpace(uiApiBaseUrl))
    {
        ui.WithEnvironment("CRONIQ_UI_API_BASEURL", uiApiBaseUrl);
    }

    if (!string.IsNullOrWhiteSpace(uiSwaggerUiUrl))
    {
        ui.WithEnvironment("CRONIQ_UI_SWAGGER_UI_URL", uiSwaggerUiUrl);
    }

    if (!string.IsNullOrWhiteSpace(uiDefaultTenantId))
    {
        ui.WithEnvironment("CRONIQ_UI_DEFAULT_TENANT_ID", uiDefaultTenantId);
    }

    if (!string.IsNullOrWhiteSpace(uiActivityStreamMode))
    {
        ui.WithEnvironment("CRONIQ_UI_WEBHOOKS_ACTIVITY_STREAM_MODE", uiActivityStreamMode);
    }

    if (!string.IsNullOrWhiteSpace(uiActivityGrpcBaseUrl))
    {
        ui.WithEnvironment("CRONIQ_UI_WEBHOOKS_ACTIVITY_GRPC_BASEURL", uiActivityGrpcBaseUrl);
    }

    if (!string.IsNullOrWhiteSpace(uiActivitySseBaseUrl))
    {
        ui.WithEnvironment("CRONIQ_UI_WEBHOOKS_ACTIVITY_SSE_BASEURL", uiActivitySseBaseUrl);
    }
}

var worker = builder.AddProject(
        "croniq-worker",
        Path.Combine(repoRoot, "samples", "Croniq.Sample.WorkerHost", "Croniq.Sample.WorkerHost.csproj"))
    .WithEnvironment("DOTNET_ENVIRONMENT", dotnetEnvironment)
    .WithEnvironment("Croniq__Core__TenantMode", tenantMode)
    .WithEnvironment("Croniq__Core__TenantId", tenantId)
    .WithEnvironment("Croniq__Core__EnvironmentTag", environmentTag)
    .WithEnvironment("Croniq__Core__InstanceId", workerInstanceId)
    .WithEnvironment("Croniq__Webhooks__Mode", "Remote")
    .WithEnvironment("Croniq__Webhooks__Remote__BaseUrl", dmzBaseUrl)
    .WithEnvironment("Croniq__Webhooks__Remote__ApiKey", dmzApiKey)
    .WithEnvironment("Croniq__Webhooks__Remote__StreamMode", "Grpc")
    .WithEnvironment("Croniq__Webhooks__Remote__StreamFallback", "Sse")
    .WithEnvironment("Croniq__Webhooks__Remote__EnableRelay", "true")
    .WithEnvironment("Croniq__Webhooks__Remote__AllowInvalidServerCertificate", "true")
    .WithEnvironment("Croniq__Logging__Execution__BasePath", logsPath)
    .WaitForCompletion(migrator, exitCode: 0);

if (usePostgres)
{
    worker.WithEnvironment("Croniq__Persistence__Mode", "Postgres");
    if (!string.IsNullOrWhiteSpace(postgresConnection))
    {
        worker.WithEnvironment("Croniq__Postgres__ConnectionString", postgresConnection);
    }
}
else if (!string.IsNullOrWhiteSpace(sqlConnection))
{
    worker.WithEnvironment("Croniq__Persistence__Mode", "SqlServer")
        .WithEnvironment("Croniq__SqlServer__ConnectionString", sqlConnection);
}

if (!string.IsNullOrWhiteSpace(otlpEndpoint))
{
    worker.WithEnvironment("Croniq__Observability__OtlpEndpoint", otlpEndpoint);
}

if (!string.IsNullOrWhiteSpace(otlpProtocol))
{
    worker.WithEnvironment("Croniq__Observability__OtlpProtocol", otlpProtocol);
}

var dmz = builder.AddProject(
        "croniq-dmz",
        Path.Combine(repoRoot, "samples", "Croniq.Sample.Dmz", "Croniq.Sample.Dmz.csproj"),
        options =>
        {
            options.ExcludeLaunchProfile = true;
            options.ExcludeKestrelEndpoints = true;
        })
    .WithHttpEndpoint(targetPort: dmzHttpPort, port: dmzHttpPort, name: "http", env: null, isProxied: false)
    .WithHttpsEndpoint(targetPort: dmzGrpcPort, port: dmzGrpcPort, name: "https", env: null, isProxied: false)
    .WithEnvironment("DOTNET_ENVIRONMENT", dotnetEnvironment)
    .WithEnvironment("ASPNETCORE_ENVIRONMENT", dotnetEnvironment)
    .WithEnvironment("ASPNETCORE_URLS", dmzUrls)
    .WithEnvironment("Croniq__Auth__Mode", dmzAuthMode)
    .WithEnvironment("Croniq__Auth__InMemory__ApiKey", dmzApiKey)
    .WithEnvironment("Croniq__Auth__InMemory__TenantId", tenantId)
    .WithEnvironment("Croniq__Auth__InMemory__EnvironmentTag", environmentTag)
    .WithEnvironment("Croniq__Persistence__Mode", "SqlServer")
    .WithEnvironment("Croniq__Core__TenantId", tenantId)
    .WithEnvironment("Croniq__Core__TenantMode", tenantMode)
    .WithEnvironment("Croniq__Core__EnvironmentTag", environmentTag)
    .WithEnvironment("Croniq__Core__InstanceId", dmzInstanceId)
    .WithEnvironment("Croniq__SqlServer__ConnectionString", dmzSqlConnection)
    .WaitForCompletion(dmzMigrator, exitCode: 0);

if (!string.IsNullOrWhiteSpace(caddyDmzUrl))
{
    dmz.WithUrlForEndpoint("http", url =>
    {
        url.Url = caddyDmzUrl;
        url.DisplayText = caddyDmzUrl;
    });

    // dmz.WithUrlForEndpoint("https", url =>
    // {
    //     url.Url = caddyDmzUrl;
    //     url.DisplayText = "Caddy (gRPC)";
    // });
}

if (!string.IsNullOrWhiteSpace(otlpEndpoint))
{
    dmz.WithEnvironment("Croniq__Observability__OtlpEndpoint", otlpEndpoint);
}

if (!string.IsNullOrWhiteSpace(otlpProtocol))
{
    dmz.WithEnvironment("Croniq__Observability__OtlpProtocol", otlpProtocol);
}

builder.Build().Run();

void EnsureDashboardEnvironment()
{
    var dashboardUrls = GetEnvValue("ASPNETCORE_URLS");
    if (string.IsNullOrWhiteSpace(dashboardUrls))
    {
        var dashboardPort = GetInt("ASPIRE_DASHBOARD_PORT", 18888);
        dashboardUrls = string.Concat("http://localhost:", dashboardPort.ToString(CultureInfo.InvariantCulture));
        Environment.SetEnvironmentVariable("ASPNETCORE_URLS", dashboardUrls);
    }

    var dashboardOtlpEndpoint = GetEnvValue("ASPIRE_DASHBOARD_OTLP_ENDPOINT_URL");
    var dashboardOtlpHttpEndpoint = GetEnvValue("ASPIRE_DASHBOARD_OTLP_HTTP_ENDPOINT_URL");
    if (string.IsNullOrWhiteSpace(dashboardOtlpEndpoint)
        && string.IsNullOrWhiteSpace(dashboardOtlpHttpEndpoint))
    {
        var dashboardOtlpPort = GetInt("ASPIRE_DASHBOARD_OTLP_PORT", 18889);
        var dashboardOtlpUrl = string.Concat(
            "http://localhost:",
            dashboardOtlpPort.ToString(CultureInfo.InvariantCulture));
        Environment.SetEnvironmentVariable("ASPIRE_DASHBOARD_OTLP_ENDPOINT_URL", dashboardOtlpUrl);
    }
}

bool IsObsEnabled(string[] args, string? profileArgs, string? obsOverride)
{
    if (IsTrue(obsOverride))
    {
        return true;
    }

    if (IsFalse(obsOverride))
    {
        return false;
    }

    return HasProfileTokens(args, "obs") || HasProfileRaw(profileArgs, "obs");
}

bool HasProfileRaw(string? rawArgs, string profile)
{
    if (string.IsNullOrWhiteSpace(rawArgs))
    {
        return false;
    }

    var tokens = rawArgs.Split(
        ' ',
        StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
    return HasProfileTokens(tokens, profile);
}

bool HasProfileTokens(IEnumerable<string> args, string profile)
{
    string? previous = null;
    foreach (var arg in args)
    {
        if (string.Equals(previous, "--profile", StringComparison.OrdinalIgnoreCase)
            && string.Equals(arg, profile, StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        if (arg.StartsWith("--profile=", StringComparison.OrdinalIgnoreCase))
        {
            var value = arg.Substring("--profile=".Length);
            if (string.Equals(value, profile, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }

        if (string.Equals(arg, profile, StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        previous = arg;
    }

    return false;
}

bool IsContainerCollectorEndpoint(string endpoint)
{
    if (Uri.TryCreate(endpoint, UriKind.Absolute, out var uri))
    {
        return string.Equals(uri.Host, "otel-collector", StringComparison.OrdinalIgnoreCase);
    }

    return endpoint.StartsWith("otel-collector", StringComparison.OrdinalIgnoreCase);
}

bool IsTrue(string? value)
{
    return string.Equals(value, "1", StringComparison.OrdinalIgnoreCase)
        || string.Equals(value, "true", StringComparison.OrdinalIgnoreCase)
        || string.Equals(value, "yes", StringComparison.OrdinalIgnoreCase);
}

bool IsFalse(string? value)
{
    return string.Equals(value, "0", StringComparison.OrdinalIgnoreCase)
        || string.Equals(value, "false", StringComparison.OrdinalIgnoreCase)
        || string.Equals(value, "no", StringComparison.OrdinalIgnoreCase);
}

static Dictionary<string, string> LoadEnvFile(string path)
{
    if (!File.Exists(path))
    {
        return new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
    }

    var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
    foreach (var line in File.ReadAllLines(path))
    {
        var trimmed = line.Trim();
        if (string.IsNullOrEmpty(trimmed) || trimmed.StartsWith("#", StringComparison.Ordinal))
        {
            continue;
        }

        if (trimmed.StartsWith("export ", StringComparison.OrdinalIgnoreCase))
        {
            trimmed = trimmed.Substring("export ".Length).Trim();
        }

        var separatorIndex = trimmed.IndexOf('=');
        if (separatorIndex <= 0)
        {
            continue;
        }

        var key = trimmed.Substring(0, separatorIndex).Trim();
        if (string.IsNullOrWhiteSpace(key))
        {
            continue;
        }

        var value = trimmed.Substring(separatorIndex + 1).Trim();
        values[key] = TrimQuotes(value);
    }

    return values;
}

static string TrimQuotes(string value)
{
    if (value.Length < 2)
    {
        return value;
    }

    var first = value[0];
    var last = value[^1];
    if ((first == '"' && last == '"') || (first == '\'' && last == '\''))
    {
        return value.Substring(1, value.Length - 2);
    }

    return value;
}

static string? FindRepoRoot(string startDirectory)
{
    var current = new DirectoryInfo(startDirectory);
    while (current is not null)
    {
        if (File.Exists(Path.Combine(current.FullName, "Directory.Build.props")))
        {
            return current.FullName;
        }

        current = current.Parent;
    }

    return null;
}

sealed class SqlServerReadyHealthCheck : IHealthCheck
{
    private readonly string _connectionString;

    public SqlServerReadyHealthCheck(string connectionString)
    {
        _connectionString = connectionString;
    }

    public async Task<HealthCheckResult> CheckHealthAsync(
        HealthCheckContext context,
        CancellationToken cancellationToken = default)
    {
        try
        {
            var builder = new SqlConnectionStringBuilder(_connectionString)
            {
                InitialCatalog = "master",
                ConnectTimeout = 2
            };

            await using var connection = new SqlConnection(builder.ConnectionString);
            await connection.OpenAsync(cancellationToken).ConfigureAwait(false);

            await using var command = connection.CreateCommand();
            command.CommandText = "SELECT 1";
            command.CommandTimeout = builder.ConnectTimeout;
            await command.ExecuteScalarAsync(cancellationToken).ConfigureAwait(false);

            return HealthCheckResult.Healthy();
        }
        catch (Exception ex)
        {
            return new HealthCheckResult(
                context.Registration.FailureStatus,
                "SQL Server not ready.",
                ex);
        }
    }
}
