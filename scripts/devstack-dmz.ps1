[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [int]$HttpPort,

    [Parameter(Mandatory = $true)]
    [int]$GrpcPort,

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

# Set ASP.NET Core URLs (HTTP + HTTPS for gRPC).
$env:ASPNETCORE_URLS = "https://0.0.0.0:$GrpcPort;http://0.0.0.0:$HttpPort"

if ($env:CRONIQ_DOTNET_ENVIRONMENT) { $env:DOTNET_ENVIRONMENT = $env:CRONIQ_DOTNET_ENVIRONMENT }

$env:Croniq__Auth__Mode = Get-EnvOrDotenv 'CRONIQ_SAMPLE_DMZ_AUTH_MODE' 'InMemory'
$env:Croniq__Auth__InMemory__ApiKey = Get-EnvOrDotenv 'CRONIQ_SAMPLE_DMZ_API_KEY' 'dmz-sample-key'
$env:Croniq__Auth__InMemory__TenantId = Get-EnvOrDotenv 'CRONIQ_CORE_TENANT_ID' 'default'
$env:Croniq__Auth__InMemory__EnvironmentTag = Get-EnvOrDotenv 'CRONIQ_ENVIRONMENT' 'dev'

$env:Croniq__Persistence__Mode = 'SqlServer'

$env:Croniq__Core__TenantId = Get-EnvOrDotenv 'CRONIQ_CORE_TENANT_ID' 'default'
$env:Croniq__Core__TenantMode = Get-EnvOrDotenv 'CRONIQ_CORE_TENANT_MODE' ''
$env:Croniq__Core__EnvironmentTag = Get-EnvOrDotenv 'CRONIQ_ENVIRONMENT' 'dev'
$env:Croniq__Core__InstanceId = Get-EnvOrDotenv 'CRONIQ_SAMPLE_DMZ_INSTANCE_ID' 'dmz-dev'

# Construct SQL Connection String
$sqlPort = Get-EnvOrDotenv 'CRONIQ_SQL_HOST_PORT' '11433'
$sqlDb = Get-EnvOrDotenv 'CRONIQ_SAMPLE_DMZ_SQL_DATABASE' 'CroniqDmz'
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

Write-Host "[devstack] Dmz terminal PID written to $PidFile" -ForegroundColor DarkGray
Write-Host "[devstack] Starting Dmz on http://localhost:$HttpPort (gRPC https://localhost:$GrpcPort)..." -ForegroundColor Cyan
Write-Host "[devstack] SQL Connection: Server=localhost,$sqlPort;Database=$sqlDb..." -ForegroundColor DarkGray

dotnet run --project samples\Croniq.Sample.Dmz\Croniq.Sample.Dmz.csproj
