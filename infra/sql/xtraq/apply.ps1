Param(
    [string]$Server = "localhost,11433",
    [string]$Database = "CroniqDev",
    [string]$User = "sa",
    [string]$Password = $env:MSSQL_SA_PASSWORD,
    [switch]$TrustServerCertificate = $true
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Try to load .env in this folder if password not provided
$root = Split-Path -Parent $MyInvocation.MyCommand.Definition
$dotenvPath = Join-Path $root ".env"
if (-not $Password -and (Test-Path $dotenvPath)) {
    Get-Content $dotenvPath | ForEach-Object {
        if ($_ -match '^\s*#') { return }
        if ($_ -match '^\s*$') { return }
        $parts = $_ -split "=", 2
        if ($parts.Length -eq 2) {
            $name = $parts[0].Trim()
            $value = $parts[1].Trim()
            if ([string]::IsNullOrWhiteSpace($name)) { return }
            $existing = [Environment]::GetEnvironmentVariable($name, 'Process')
            if (-not [string]::IsNullOrWhiteSpace($existing)) { return }
            Set-Item -Path Env:$name -Value $value -ErrorAction SilentlyContinue
        }
    }
    if (-not $Password -and $env:MSSQL_SA_PASSWORD) { $Password = $env:MSSQL_SA_PASSWORD }
    if (-not $Password -and $env:SA_PASSWORD) { $Password = $env:SA_PASSWORD }
}

if (-not $Password -and -not $PSBoundParameters.ContainsKey("User")) {
    throw "Password required. Provide -Password or set MSSQL_SA_PASSWORD/SA_PASSWORD (or define them in infra/sql/xtraq/.env)."
}

$sqlcmdPath = Get-Command sqlcmd -ErrorAction SilentlyContinue
if (-not $sqlcmdPath) {
    throw "sqlcmd not found. Install the SQLCMD CLI (sqlcmd-cli or SQL Server client tools) and ensure it is on PATH."
}

function Get-SqlcmdArgs([string]$dbName, [string]$inputFile, [string]$query) {
    $args = @("-S", $Server, "-b", "-l", "30")
    if ($TrustServerCertificate) { $args += @("-N", "-C") }
    if ($User) {
        if (-not $Password) { throw "Password required for user '$User'." }
        $args += @("-U", $User, "-P", $Password)
    } else {
        $args += "-E"
    }
    if ($dbName) { $args += @("-d", $dbName) }
    if ($inputFile) { $args += @("-i", $inputFile) }
    if ($query) { $args += @("-Q", $query) }
    return ,$args
}

Write-Host "Ensuring database [$Database] exists on $Server..."
& $sqlcmdPath @((Get-SqlcmdArgs -dbName "master" -query "IF DB_ID('$Database') IS NULL CREATE DATABASE [$Database];"))

$scripts = @(
    "predeploy.sql",
    "core/types.sql",
    "core/procs.health.sql",
    "core-internal/types.sql",
    "core-internal/procs.errors.sql",
    "core-internal/procs.guards.sql",
    "croniq/types.sql",
    "croniq/functions.sql",
    "croniq-internal/types.sql",
    "croniq-internal/procs.errors.sql",
    "croniq-internal/procs.guards.sql",
    "auth/types.sql",
    "auth/tables.sql",
    "auth/procs.keys.sql",
    "croniq/tables.sql",
    "croniq/procs.instances.sql",
    "croniq/procs.jobs.sql",
    "croniq/procs.leases.sql",
    "croniq/procs.deadletter.sql"
)

foreach ($relative in $scripts) {
    $path = Join-Path $root $relative
    if (-not (Test-Path $path)) {
        throw "Missing script: $relative"
    }

    Write-Host "Applying $relative..."
    & $sqlcmdPath @((Get-SqlcmdArgs -dbName $Database -inputFile $path))
}

Write-Host "All scripts applied to [$Database] on $Server."
