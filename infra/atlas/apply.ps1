param(
    [string]$EnvFile = ".env",
    [string]$DacpacOutput = "schema.dacpac",
    [switch]$SkipExtract = $false,
    [switch]$UseDockerSqlcmd = $false,
    [string]$SqlContainer = "croniq-mssql",
    [string]$SqlPackagePath = "sqlpackage"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Import-Env {
    param([string]$Path)

    $resolved = $Path
    if (-not (Split-Path -IsAbsolute $Path)) {
        $resolved = Join-Path $PSScriptRoot $Path
    }
    if (-not (Test-Path $resolved)) {
        throw "Env file not found: $resolved"
    }

    Get-Content $resolved | ForEach-Object {
        if ($_ -match "^\s*$" -or $_ -like "#*") { return }
        $parts = $_ -split "=", 2
        if ($parts.Length -eq 2) {
            $name = $parts[0]
            $value = $parts[1].Trim()
            Set-Item -Path "env:$name" -Value $value
        }
    }
}

Import-Env -Path $EnvFile

$serverHost = "localhost,$($env:MSSQL_PORT)"
$serverDocker = "localhost,1433"
$db = $env:MSSQL_DB
$user = $env:MSSQL_USER
$pwd = $env:SA_PASSWORD

if (-not $db) { throw "MSSQL_DB not set" }
if (-not $pwd) { throw "SA_PASSWORD not set" }
if (-not $user) { throw "MSSQL_USER not set" }

$sqlRoot = Join-Path $PSScriptRoot "..\sql\xtraq"
$sqlCmdPath = "sqlcmd"
$trustFlag = "-C"
if ($UseDockerSqlcmd) {
    $sqlCmdPath = "docker exec -i $SqlContainer /opt/mssql-tools18/bin/sqlcmd"
}

function Invoke-Sql {
    param(
        [string]$Database,
        [string]$File,
        [string]$Query
    )
    $targetServer = "localhost,$($env:MSSQL_PORT)"
    if ($UseDockerSqlcmd) {
        $targetServer = "localhost,1433"
    }

    if ($UseDockerSqlcmd -and $File) {
        $tempPath = "/tmp/script.sql"
        $content = Get-Content -Raw $File
        $copyCmd = "docker exec -i $SqlContainer /bin/bash -c `"cat > $tempPath`""
        $content | & cmd /c $copyCmd

        $cmd = "docker exec -i $SqlContainer /opt/mssql-tools18/bin/sqlcmd -S $targetServer -U $user -P $pwd $trustFlag"
        if ($Database) { $cmd += " -d $Database" }
        $cmd += " -i $tempPath"
        Invoke-Expression $cmd
    }
    else {
        $cmd = $sqlCmdPath
        $cmd += " -S $targetServer -U $user -P $pwd $trustFlag"
        if ($Database) {
            $cmd += " -d $Database"
        }
        if ($File) {
            $cmd += " -i `"$File`""
        }
        if ($Query) {
            $cmd += " -Q `"$Query`""
        }
        Invoke-Expression $cmd
    }
}

$filesInOrder = @(
    (Join-Path $sqlRoot "core\types.sql"),
    (Join-Path $sqlRoot "croniq\types.sql"),
    (Join-Path $sqlRoot "auth\tables.sql"),
    (Join-Path $sqlRoot "croniq\tables.sql"),
    (Join-Path $sqlRoot "croniq\procs.instances.sql"),
    (Join-Path $sqlRoot "croniq\procs.jobs.sql"),
    (Join-Path $sqlRoot "croniq\procs.leases.sql"),
    (Join-Path $sqlRoot "croniq\procs.deadletter.sql")
)

$reachable = $false
for ($i = 0; $i -lt 20; $i++) {
    try {
        Invoke-Sql -Database "master" -Query "SELECT 1" | Out-Null
        if ($LASTEXITCODE -eq 0) { $reachable = $true; break }
    } catch {
    }
    Start-Sleep -Seconds 3
}
if (-not $reachable) {
    throw "SQL Server not reachable. Check container health, port, and credentials."
}

Write-Host "Ensuring database '$db' exists..."
Invoke-Sql -Database "master" -Query "IF DB_ID('$db') IS NULL BEGIN CREATE DATABASE [$db]; END"

foreach ($file in $filesInOrder) {
    Write-Host "Applying $file"
    Invoke-Sql -Database $db -File $file
}

if (-not $SkipExtract) {
    Write-Host "Extracting dacpac to $DacpacOutput"
    $targetDac = Join-Path $PSScriptRoot $DacpacOutput
    $extractCmd = "`"$SqlPackagePath`" /Action:Extract /SourceServerName:`"$serverHost`" /SourceDatabaseName:`"$db`" /SourceUser:`"$user`" /SourcePassword:`"$pwd`" /SourceTrustServerCertificate:True /SourceEncrypt:False /OverwriteFiles:True /TargetFile:`"$targetDac`""
    Invoke-Expression $extractCmd
}

Write-Host "Done."
