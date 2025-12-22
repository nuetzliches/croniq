[CmdletBinding()]
param(
    [string]$Configuration = "Release",
    [string]$SqlConnection = "Server=localhost,11433;Database=CroniqDev;User Id=sa;Password=CroniqSqlP@ssw0rd!;Encrypt=False;TrustServerCertificate=True;",
    [switch]$DisableCoverage,
    [bool]$NoRestore = $true,
    [string[]]$Projects = @(),
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

function Resolve-TestProjects {
    param(
        [string]$RepoRoot,
        [string[]]$RequestedProjects
    )

    $testsRoot = Join-Path $RepoRoot "tests"
    if (-not (Test-Path $testsRoot)) {
        throw "Could not find tests directory at '$testsRoot'."
    }

    if ($RequestedProjects -and $RequestedProjects.Count -gt 0) {
        $resolved = @()
        foreach ($p in $RequestedProjects) {
            $candidate = $p
            if (-not [System.IO.Path]::IsPathRooted($candidate)) {
                $candidate = Join-Path $RepoRoot $candidate
            }

            $candidate = Resolve-Path $candidate -ErrorAction Stop
            if (-not (Test-Path $candidate)) {
                throw "Project path '$p' does not exist (resolved: '$candidate')."
            }

            $resolved += $candidate
        }

        return $resolved
    }

    $all = Get-ChildItem -Path $testsRoot -Filter "*.csproj" -Recurse -File | Sort-Object FullName

    # Exclude shared test utilities.
    $all = $all | Where-Object { $_.Name -ne "Croniq.TestKit.csproj" }

    return $all.FullName
}

$originalLocation = Get-Location
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

if ([System.String]::Equals($SqlConnection, $defaultDevstackSql, [System.StringComparison]::OrdinalIgnoreCase)) {
    Ensure-DevstackSql -ConnectionString $SqlConnection -RepoRoot $repoRoot
}

$artifactsRoot = Join-Path $repoRoot "artifacts"
$ciRoot = Join-Path $artifactsRoot "ci"
$testResultsRoot = Join-Path $ciRoot "tests-individual"
$coverageReportDir = Join-Path $ciRoot "coverage-report"
$historyDir = Join-Path $coverageReportDir "history"

$null = New-Item -ItemType Directory -Path $testResultsRoot -Force
$null = New-Item -ItemType Directory -Path $coverageReportDir -Force
$null = New-Item -ItemType Directory -Path $historyDir -Force

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
    $testProjects = Resolve-TestProjects -RepoRoot $repoRoot -RequestedProjects $Projects
    if (-not $testProjects -or $testProjects.Count -eq 0) {
        throw "No test projects found."
    }

    Write-Host "Running tests individually ($($testProjects.Count) project(s)) to avoid VS Code hangs from solution-wide output..." -ForegroundColor Cyan

    $overallExitCode = 0

    foreach ($projectPath in $testProjects) {
        $projectName = [System.IO.Path]::GetFileNameWithoutExtension($projectPath)
        $projectResultsDir = Join-Path $testResultsRoot $projectName
        $null = New-Item -ItemType Directory -Path $projectResultsDir -Force

        $trxName = "release-tests.trx"
        $binlogPath = Join-Path $projectResultsDir "dotnet-test.binlog"
        $vstestDiagPath = Join-Path $projectResultsDir "vstest.diag"

        $dotnetArgs = @(
            "test",
            $projectPath,
            "-c", $Configuration,
            "--nologo",
            "--verbosity", "minimal",
            "--logger", "trx;LogFileName=$trxName",
            "--logger", "console;verbosity=minimal",
            "--results-directory", $projectResultsDir,
            "/bl:$binlogPath",
            "/p:VSTestDiag=$vstestDiagPath"
        )

        if ($NoRestore) {
            $dotnetArgs += "--no-restore"
        }

        if ($DisableCoverage) {
            $dotnetArgs += "/p:CollectCoverage=false"
        }
        else {
            $dotnetArgs += "/p:CollectCoverage=true"
        }

        if ($AdditionalDotnetArguments.Count -gt 0) {
            $dotnetArgs += $AdditionalDotnetArguments
        }

        Write-Host "Running $projectName..." -ForegroundColor Cyan
        Write-Host "dotnet $($dotnetArgs -join ' ')" -ForegroundColor DarkGray

        & dotnet @dotnetArgs
        $exitCode = $LASTEXITCODE
        if ($exitCode -ne 0) {
            $overallExitCode = $exitCode
            Write-Warning "${projectName}: dotnet test exited with code $exitCode"
        }
    }

    if (-not $DisableCoverage) {
        $coverageFiles = Get-ChildItem -Path $repoRoot -Filter "coverage.cobertura.xml" -Recurse -ErrorAction SilentlyContinue
        if ($coverageFiles.Count -gt 0) {
            $reportsArg = ($coverageFiles.FullName -join ";")
            Write-Host "Generating aggregated coverage report from $($coverageFiles.Count) cobertura file(s)..." -ForegroundColor Cyan

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

    exit $overallExitCode
}
finally {
    foreach ($key in $previousEnv.Keys) {
        [Environment]::SetEnvironmentVariable($key, $previousEnv[$key], "Process")
    }

    Set-Location $originalLocation
}
