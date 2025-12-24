[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $Arguments = @()
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$targetScript = Join-Path $repoRoot "scripts\run-tests.ps1"

if (-not (Test-Path $targetScript)) {
    throw "Could not find '$targetScript'."
}

$pwsh = "pwsh"
$argList = @(
    "-NoProfile",
    "-ExecutionPolicy", "Bypass",
    "-File", $targetScript
) + $Arguments

Write-Host "Launching full test run in a separate terminal window..." -ForegroundColor Cyan
Start-Process -FilePath $pwsh -WorkingDirectory $repoRoot -ArgumentList $argList | Out-Null
