[CmdletBinding()]
param(
    [string]$Configuration = "Debug",
    [string]$Solution = "croniq.slnx",
    [string]$SqlConnection = "Server=localhost,11433;Database=CroniqDev;User Id=sa;Password=CroniqSqlP@ssw0rd!;Encrypt=False;TrustServerCertificate=True;",
    [switch]$DisableCoverage,
    [string[]]$AdditionalDotnetArguments = @()
)

$ErrorActionPreference = "Stop"

# Keep in sync with the default value of -SqlConnection above to detect when we can auto-bootstrap the devstack.
$defaultDevstackSql = "Server=localhost,11433;Database=CroniqDev;User Id=sa;Password=CroniqSqlP@ssw0rd!;Encrypt=False;TrustServerCertificate=True;"

$composeFileRelativePaths = @(
    "infra\docker\docker-compose.yml",
    "infra\docker\docker-compose.dev.yml",
    "infra\docker\docker-compose.observability.yml"
)

function Test-SqlConnectivity {
    param(
        [string]$ConnectionString
    )

    try {
        $builder = New-Object System.Data.SqlClient.SqlConnectionStringBuilder $ConnectionString
        if (-not [string]::IsNullOrWhiteSpace($builder.InitialCatalog)) {
            $builder["Initial Catalog"] = "master"
        }

        $builder["Connect Timeout"] = 3
        $connection = New-Object System.Data.SqlClient.SqlConnection $builder.ConnectionString
        $connection.Open()
        $connection.Dispose()
        return $true
    }
    catch {
        return $false
    }
}

function Start-DevstackSql {
    param(
        [string]$RepoRoot
    )

    $envFile = Join-Path $RepoRoot ".env"
    if (-not (Test-Path $envFile)) {
        throw "Croniq devstack requires a .env file in the repo root. Copy .env.example to .env first."
    }

    $composeArgs = @("compose")
    foreach ($relative in $composeFileRelativePaths) {
        $fullPath = Join-Path $RepoRoot $relative
        if (-not (Test-Path $fullPath)) {
            throw "Required docker compose file '$relative' could not be found."
        }

        $composeArgs += @("-f", $fullPath)
    }

    $composeArgs += @("--profile", "sql", "up", "-d", "--quiet-pull")
    Write-Host "Croniq devstack SQL unreachable. Bootstrapping via docker compose (profile sql)..." -ForegroundColor Yellow
    & docker @composeArgs
    if ($LASTEXITCODE -ne 0) {
        throw "docker compose up (sql profile) failed with exit code $LASTEXITCODE."
    }
}

function Ensure-DevstackSql {
    param(
        [string]$ConnectionString,
        [string]$RepoRoot
    )

    if (Test-SqlConnectivity -ConnectionString $ConnectionString) {
        return
    }

    Start-DevstackSql -RepoRoot $RepoRoot

    $deadline = (Get-Date).AddMinutes(2)
    while ((Get-Date) -lt $deadline) {
        if (Test-SqlConnectivity -ConnectionString $ConnectionString) {
            Write-Host "Croniq devstack SQL endpoint is reachable." -ForegroundColor Green
            return
        }

        Start-Sleep -Seconds 3
    }

    throw "Croniq devstack SQL endpoint is still unavailable after docker compose up."
}

$originalLocation = Get-Location
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

if ([System.String]::Equals($SqlConnection, $defaultDevstackSql, [System.StringComparison]::OrdinalIgnoreCase)) {
    Ensure-DevstackSql -ConnectionString $SqlConnection -RepoRoot $repoRoot
}

$artifactsRoot = Join-Path $repoRoot "artifacts"
$ciRoot = Join-Path $artifactsRoot "ci"
$testResultsDir = Join-Path $ciRoot "tests"
$coverageReportDir = Join-Path $ciRoot "coverage-report"
$historyDir = Join-Path $coverageReportDir "history"

$null = New-Item -ItemType Directory -Path $testResultsDir -Force
$null = New-Item -ItemType Directory -Path $coverageReportDir -Force
$null = New-Item -ItemType Directory -Path $historyDir -Force

$binlogPath = Join-Path $ciRoot "dotnet-test.binlog"
$vstestDiagPath = Join-Path $ciRoot "vstest.diag"
$trxName = "Croniq.Tests.trx"

Write-Host "Restoring local dotnet tools..." -ForegroundColor Cyan
& dotnet tool restore
if ($LASTEXITCODE -ne 0) {
    throw "dotnet tool restore failed with exit code $LASTEXITCODE."
}

$envOverrides = @{
    "CRONIQ_SQL"                                       = $SqlConnection
    "DOTNET_NOLOGO"                                    = "1"
    "DOTNET_CLI_UI_LANGUAGE"                           = "en"
    "DOTNET_PRINT_TELEMETRY_MESSAGE"                   = "false"
    "DOTNET_ENVIRONMENT"                               = "CI"
    "ASPNETCORE_ENVIRONMENT"                           = "CI"
    "Logging__LogLevel__Default"                       = "Warning"
    "Logging__LogLevel__Microsoft"                     = "Warning"
    "Logging__LogLevel__Microsoft.EntityFrameworkCore" = "Warning"
}

$previousEnv = @{}
foreach ($entry in $envOverrides.GetEnumerator()) {
    $previousEnv[$entry.Key] = [Environment]::GetEnvironmentVariable($entry.Key, "Process")
    [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, "Process")
}

try {
    $dotnetArgs = @(
        "test",
        $Solution,
        "-c", $Configuration,
        "--nologo",
        "--verbosity", "minimal",
        "--logger", "trx;LogFileName=$trxName",
        "--logger", "console;verbosity=minimal",
        "--results-directory", $testResultsDir,
        "/bl:$binlogPath",
        "/p:VSTestDiag=$vstestDiagPath"
    )

    if ($DisableCoverage) {
        $dotnetArgs += "/p:CollectCoverage=false"
    }
    else {
        $dotnetArgs += "/p:CollectCoverage=true"
    }

    if ($AdditionalDotnetArguments.Count -gt 0) {
        $dotnetArgs += $AdditionalDotnetArguments
    }

    Write-Host "Running dotnet $($dotnetArgs -join ' ')" -ForegroundColor Cyan
    & dotnet @dotnetArgs
    $testExitCode = $LASTEXITCODE

    if ($testExitCode -ne 0) {
        Write-Warning "dotnet test exited with code $testExitCode"
    }

    if (-not $DisableCoverage) {
        $coverageFiles = Get-ChildItem -Path $repoRoot -Filter "coverage.cobertura.xml" -Recurse -ErrorAction SilentlyContinue
        if ($coverageFiles.Count -gt 0) {
            $reportsArg = ($coverageFiles.FullName -join ";")
            Write-Host "Generating coverage report from $($coverageFiles.Count) cobertura file(s)..." -ForegroundColor Cyan

            if (Test-Path $coverageReportDir) {
                Get-ChildItem -Path $coverageReportDir -Recurse -Force | Remove-Item -Force -Recurse -ErrorAction SilentlyContinue
                $null = New-Item -ItemType Directory -Path $coverageReportDir -Force
                $null = New-Item -ItemType Directory -Path $historyDir -Force
            }

            $reportArgs = @(
                "-reports:$reportsArg",
                "-targetdir:$coverageReportDir",
                "-reporttypes:Html;Cobertura",
                "-historydir:$historyDir"
            )

            & dotnet tool run reportgenerator @reportArgs
            $reportExitCode = $LASTEXITCODE
            if ($reportExitCode -ne 0) {
                Write-Warning "reportgenerator exited with code $reportExitCode"
            }
        }
        else {
            Write-Warning "No coverage.cobertura.xml files were produced."
        }
    }

    exit $testExitCode
}
finally {
    foreach ($key in $previousEnv.Keys) {
        [Environment]::SetEnvironmentVariable($key, $previousEnv[$key], "Process")
    }

    Set-Location $originalLocation
}
