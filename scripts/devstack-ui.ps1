[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [int]$UiPort,

    [Parameter()]
    [string]$UiDir = 'src\Croniq.Ui',

    [Parameter()]
    [string]$PidFile = 'artifacts\devstack\ui.pid'
)

$ErrorActionPreference = 'Stop'

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location -LiteralPath $repoRoot

$pidDir = Split-Path -Parent $PidFile
if ($pidDir -and -not (Test-Path -LiteralPath $pidDir)) {
    New-Item -ItemType Directory -Path $pidDir | Out-Null
}

# Write PID early so devstack-down can terminate this terminal session.
$PID | Out-File -FilePath $PidFile -Encoding ascii

Write-Host "[devstack] UI terminal PID written to $PidFile" -ForegroundColor DarkGray

$uiPath = Join-Path $repoRoot $UiDir
if (-not (Test-Path -LiteralPath (Join-Path $uiPath 'package.json'))) {
    throw "UI not started: $UiDir\\package.json not found."
}

Set-Location -LiteralPath $uiPath

Write-Host "[devstack] Running: npm install" -ForegroundColor DarkGray
npm install

Write-Host "[devstack] Running: npm run generate:api:server:snapshot" -ForegroundColor DarkGray
npm run generate:api:server:snapshot

Write-Host "[devstack] Starting: npm start -- --port $UiPort" -ForegroundColor DarkGray
npm start "--" --port $UiPort
