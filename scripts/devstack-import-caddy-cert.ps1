[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter()]
    [string]$ContainerName,

    [Parameter()]
    [string]$ContainerCertPath = '/data/caddy/pki/authorities/local/root.crt',

    [Parameter()]
    [string]$CertPath = 'artifacts\caddy-root.crt',

    [Parameter()]
    [switch]$CopyOnly
)

$ErrorActionPreference = 'Stop'

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location -LiteralPath $repoRoot

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw 'docker is required to copy the Caddy root certificate.'
}

if ([string]::IsNullOrWhiteSpace($ContainerName)) {
    $names = @(
        & docker ps --filter 'name=caddy' --format '{{.Names}}' |
            ForEach-Object { $_.Trim() } |
            Where-Object { $_ }
    )
    if ($names.Count -eq 0) {
        throw 'No running Caddy container found. Start the devstack first.'
    }
    $ContainerName = $names[0]
    if ($names.Count -gt 1) {
        Write-Host "[devstack] Multiple Caddy containers found. Using $ContainerName." -ForegroundColor DarkGray
    }
}

if ([string]::IsNullOrWhiteSpace($ContainerName)) {
    throw 'Caddy container name resolved to empty. Provide -ContainerName explicitly.'
}

if ([string]::IsNullOrWhiteSpace($ContainerCertPath)) {
    throw 'Container cert path is required.'
}

$certFullPath = Join-Path $repoRoot $CertPath
$certDir = Split-Path -Parent $certFullPath
if ($certDir -and -not (Test-Path -LiteralPath $certDir)) {
    New-Item -ItemType Directory -Path $certDir | Out-Null
}

Write-Host "[devstack] Copying Caddy root cert from $ContainerName..." -ForegroundColor DarkGray
& docker cp "$ContainerName`:$ContainerCertPath" $certFullPath
if ($LASTEXITCODE -ne 0) {
    throw "docker cp failed with exit code $LASTEXITCODE."
}

if (-not (Test-Path -LiteralPath $certFullPath -PathType Leaf)) {
    throw "Caddy root certificate was not copied to $certFullPath."
}

if (-not $CopyOnly) {
    if (-not (Test-IsAdmin)) {
        throw 'Run this script as Administrator to trust the certificate (LocalMachine\\Root).'
    }

    if (-not (Get-Command certutil -ErrorAction SilentlyContinue)) {
        throw 'certutil not found. Install or use Import-Certificate to trust the CA.'
    }

    if ($PSCmdlet.ShouldProcess('LocalMachine\\Root', "Import $certFullPath")) {
        & certutil -addstore -f Root $certFullPath | Out-Null
    }

    Write-Host "[devstack] Caddy root certificate trusted in LocalMachine\\Root." -ForegroundColor Cyan
}
else {
    Write-Host "[devstack] Caddy root certificate copied to $certFullPath." -ForegroundColor Cyan
}
