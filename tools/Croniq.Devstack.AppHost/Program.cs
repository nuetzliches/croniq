using System.Globalization;

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
        return "localhost";
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

var api = builder.AddProject(
        "croniq-api",
        Path.Combine(repoRoot, "samples", "Croniq.Sample.ApiHost", "Croniq.Sample.ApiHost.csproj"))
    .WithEnvironment("DOTNET_ENVIRONMENT", dotnetEnvironment)
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
        Path.Combine(repoRoot, "samples", "Croniq.Sample.Dmz", "Croniq.Sample.Dmz.csproj"))
    .WithEnvironment("DOTNET_ENVIRONMENT", dotnetEnvironment)
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

    return HasProfile(args, "obs") || HasProfile(profileArgs, "obs");
}

bool HasProfile(string? rawArgs, string profile)
{
    if (string.IsNullOrWhiteSpace(rawArgs))
    {
        return false;
    }

    var tokens = rawArgs.Split(
        ' ',
        StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
    return HasProfile(tokens, profile);
}

bool HasProfile(IEnumerable<string> args, string profile)
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
