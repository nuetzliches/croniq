[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [int]$Port,

    [Parameter(Mandatory = $true)]
    [string]$PidFile,

    [Parameter()]
    [string]$OtlpEndpoint,

    [Parameter()]
    [string]$OtlpProtocol
)

$ErrorActionPreference = 'Stop'

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location -LiteralPath $repoRoot

# --- Environment Setup ---

# Helper to get env var, then .env value, then fallback
$dotenv = @{}
if (Test-Path '.env') {
    Get-Content '.env' | ForEach-Object {
        $line = $_.Trim()
        if (-not $line -or $line.StartsWith('#')) { return }
        $parts = $line -split '=', 2
        if ($parts.Count -eq 2) {
            $dotenv[$parts[0].Trim()] = $parts[1].Trim()
        }
    }
}

function Get-EnvOrDotenv([string]$key, [string]$fallback) {
    $val = [Environment]::GetEnvironmentVariable($key)
    if ($val) { return $val }
    if ($dotenv.ContainsKey($key) -and $dotenv[$key]) { return $dotenv[$key] }
    return $fallback
}

# Set ASP.NET Core URL
$env:ASPNETCORE_URLS = "http://0.0.0.0:$Port"

# Map Croniq env vars
if ($env:CRONIQ_DOTNET_ENVIRONMENT) { $env:DOTNET_ENVIRONMENT = $env:CRONIQ_DOTNET_ENVIRONMENT }

$authMode = Get-EnvOrDotenv 'CRONIQ_AUTH_MODE' ''
if ($authMode) { $env:Croniq__Auth__Mode = $authMode }

$env:Croniq__Auth__InMemory__ApiKey = Get-EnvOrDotenv 'CRONIQ_API_KEY' ''
$env:Croniq__Auth__InMemory__TenantId = Get-EnvOrDotenv 'CRONIQ_CORE_TENANT_ID' 'default'
$env:Croniq__Auth__InMemory__EnvironmentTag = Get-EnvOrDotenv 'CRONIQ_ENVIRONMENT' ''

$env:Croniq__Persistence__Mode = 'SqlServer'

$env:Croniq__Api__RequestsPerMinute = Get-EnvOrDotenv 'CRONIQ_API_REQUESTS_PER_MINUTE' ''
$env:Croniq__Core__TenantId = Get-EnvOrDotenv 'CRONIQ_CORE_TENANT_ID' 'default'
$env:Croniq__Core__TenantMode = Get-EnvOrDotenv 'CRONIQ_CORE_TENANT_MODE' ''
$env:Croniq__Core__EnvironmentTag = Get-EnvOrDotenv 'CRONIQ_ENVIRONMENT' ''
$env:Croniq__Core__InstanceId = Get-EnvOrDotenv 'CRONIQ_API_INSTANCE_ID' ''
$env:Croniq__Logging__Execution__BasePath = (Join-Path $repoRoot 'logs')

# Remote webhook ingress (DMZ simulation).
$dmzGrpcPort = Get-EnvOrDotenv 'CRONIQ_DMZ_GRPC_PORT' '5001'
$dmzBaseUrl = Get-EnvOrDotenv 'CRONIQ_DMZ_BASEURL' "https://localhost:$dmzGrpcPort"
$dmzApiKey = Get-EnvOrDotenv 'CRONIQ_DMZ_API_KEY' 'dmz-sample-key'
$env:Croniq__Webhooks__Mode = 'Remote'
$env:Croniq__Webhooks__Remote__BaseUrl = $dmzBaseUrl
$env:Croniq__Webhooks__Remote__ApiKey = $dmzApiKey
$env:Croniq__Webhooks__Remote__EnableRelay = 'true'

# Construct SQL Connection String
$sqlPort = Get-EnvOrDotenv 'CRONIQ_SQL_HOST_PORT' '11433'
$sqlDb = Get-EnvOrDotenv 'CRONIQ_SQL_DATABASE' 'CroniqDev'
$sqlPw = Get-EnvOrDotenv 'CRONIQ_SQL_PASSWORD' 'CroniqSqlP@ssw0rd!'
$env:Croniq__SqlServer__ConnectionString = "Server=localhost,$sqlPort;Database=$sqlDb;User Id=sa;Password=$sqlPw;Encrypt=False;TrustServerCertificate=True;"

# Observability
if ($OtlpEndpoint) {
    $env:Croniq__Observability__OtlpEndpoint = $OtlpEndpoint
    if ($OtlpProtocol) {
        $env:Croniq__Observability__OtlpProtocol = $OtlpProtocol
    }
}

# --- Execution ---

$pidDir = Split-Path -Parent $PidFile
if ($pidDir -and -not (Test-Path -LiteralPath $pidDir)) {
    New-Item -ItemType Directory -Path $pidDir | Out-Null
}

# Write PID so devstack-down can terminate this terminal session.
$PID | Out-File -FilePath $PidFile -Encoding ascii

Write-Host "[devstack] ApiHost terminal PID written to $PidFile" -ForegroundColor DarkGray
Write-Host "[devstack] Starting ApiHost on port $Port..." -ForegroundColor Cyan
Write-Host "[devstack] SQL Connection: Server=localhost,$sqlPort;Database=$sqlDb..." -ForegroundColor DarkGray

# Run dotnet run
dotnet run --project samples\Croniq.Sample.ApiHost\Croniq.Sample.ApiHost.csproj
