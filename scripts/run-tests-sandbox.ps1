[CmdletBinding()]
param(
    [string] $Name = "local",

    [switch] $Wait,

    [string]$Configuration = "Debug",
    [string]$Solution = "croniq.slnx",
    [string]$SqlConnection = "Server=localhost,11433;Database=CroniqDev;User Id=sa;Password=CroniqSqlP@ssw0rd!;Encrypt=False;TrustServerCertificate=True;",
    [switch]$DisableCoverage,

    [string]$Filter,

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$AdditionalDotnetArguments = @()
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$targetScript = Join-Path $repoRoot "scripts\\run-tests.ps1"

if (-not (Test-Path $targetScript)) {
    throw "Could not find '$targetScript'."
}

$runId = (Get-Date).ToString("yyyyMMdd-HHmmss")
$safeName = ($Name -replace '[^A-Za-z0-9._-]', '_')
$runDir = Join-Path $repoRoot "artifacts\\runs\\$safeName\\$runId"

New-Item -ItemType Directory -Path $runDir -Force | Out-Null

$pwsh = "pwsh"
$escapedTargetScript = $targetScript.Replace("'", "''")
$escapedRunDir = $runDir.Replace("'", "''")
$escapedSolution = $Solution.Replace("'", "''")
$escapedSql = $SqlConnection.Replace("'", "''")

if (-not [string]::IsNullOrWhiteSpace($Filter)) {
    $AdditionalDotnetArguments += @("--filter", $Filter)
}

$dotnetArgsLiteral = $null
if ($AdditionalDotnetArguments.Count -gt 0) {
    $dotnetArgsLiteral = "@(" + (($AdditionalDotnetArguments | ForEach-Object { "'" + ($_.Replace("'", "''")) + "'" }) -join ",") + ")"
}

$invoke = "& '$escapedTargetScript' -ArtifactsDirectory '$escapedRunDir' -Configuration '$Configuration' -Solution '$escapedSolution' -SqlConnection '$escapedSql'"
if ($DisableCoverage) {
    $invoke += " -DisableCoverage"
}
if ($dotnetArgsLiteral) {
    $invoke += " -AdditionalDotnetArguments $dotnetArgsLiteral"
}

$argList = @(
    "-NoProfile",
    "-ExecutionPolicy", "Bypass",
    "-Command", $invoke
)

Write-Host "Starting sandboxed test run..." -ForegroundColor Cyan
Write-Host "Run folder: $runDir" -ForegroundColor Cyan
Write-Host "Tip: Tail logs via: Get-Content -Wait '$runDir\\dotnet-test.console.log'" -ForegroundColor DarkGray

$pwshStdOut = Join-Path $runDir "pwsh.stdout.log"
$pwshStdErr = Join-Path $runDir "pwsh.stderr.log"
$process = Start-Process -FilePath $pwsh -WorkingDirectory $repoRoot -ArgumentList $argList -PassThru -RedirectStandardOutput $pwshStdOut -RedirectStandardError $pwshStdErr
Write-Host "Started PID $($process.Id). Logs will continue even if VS Code freezes." -ForegroundColor Green

if ($Wait) {
    Write-Host "Waiting for test process to finish..." -ForegroundColor Cyan
    $process.WaitForExit()
    $exitCode = $process.ExitCode
    $summaryPath = Join-Path $runDir "run-summary.txt"

    if (Test-Path $summaryPath) {
        Write-Host "--- run-summary.txt ---" -ForegroundColor Cyan
        Get-Content -Path $summaryPath
    }
    else {
        Write-Warning "run-summary.txt not found at '$summaryPath'. Listing run folder contents instead."
        Get-ChildItem -Force -Path $runDir | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize
    }

    exit $exitCode
}
