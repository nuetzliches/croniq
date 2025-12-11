[CmdletBinding()]
param(
    [string]$Configuration = "Debug",
    [string]$Solution = "croniq.sln",
    [string]$SqlConnection = "Server=localhost,11433;Database=CroniqDev;User Id=sa;Password=CroniqSqlP@ssw0rd!;Encrypt=False;TrustServerCertificate=True;",
    [switch]$DisableCoverage,
    [string[]]$AdditionalDotnetArguments = @()
)

$ErrorActionPreference = "Stop"

$originalLocation = Get-Location
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

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

$originalSql = $env:CRONIQ_SQL
$env:CRONIQ_SQL = $SqlConnection

try {
    $dotnetArgs = @(
        "test",
        $Solution,
        "-c", $Configuration,
        "--logger", "trx;LogFileName=$trxName",
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
    $env:CRONIQ_SQL = $originalSql
    Set-Location $originalLocation
}
