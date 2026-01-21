[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter()]
    [string]$Domain,

    [Parameter()]
    [string[]]$Hosts = @('api', 'dmz', 'hooks', 'ui'),

    [Parameter()]
    [string]$IpAddress = '127.0.0.1',

    [Parameter()]
    [string]$HostsFile = "$env:SystemRoot\System32\drivers\etc\hosts",

    [Parameter()]
    [switch]$NoBackup
)

$ErrorActionPreference = 'Stop'

function Test-IsAdmin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not (Test-IsAdmin)) {
    throw 'This script must run in an elevated PowerShell (Administrator) to edit the hosts file.'
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location -LiteralPath $repoRoot

$dotenv = @{}
$dotenvPath = Join-Path $repoRoot '.env'
if (Test-Path -LiteralPath $dotenvPath) {
    Get-Content -LiteralPath $dotenvPath | ForEach-Object {
        $line = $_.Trim()
        if (-not $line -or $line.StartsWith('#')) { return }
        $parts = $line -split '=', 2
        if ($parts.Count -eq 2) {
            $dotenv[$parts[0].Trim()] = $parts[1].Trim()
        }
    }
}

function Get-EnvOrDotenv([string]$key) {
    $val = [Environment]::GetEnvironmentVariable($key)
    if ($val) { return $val }
    if ($dotenv.ContainsKey($key) -and $dotenv[$key]) { return $dotenv[$key] }
    return $null
}

if (-not $PSBoundParameters.ContainsKey('Domain') -or [string]::IsNullOrWhiteSpace($Domain)) {
    $Domain = Get-EnvOrDotenv 'CRONIQ_CADDY_DOMAIN'
}

if ([string]::IsNullOrWhiteSpace($Domain)) {
    $Domain = 'croniq.local'
}

$Domain = $Domain.Trim().Trim('.')

$expandedHosts = foreach ($hostEntry in $Hosts) {
    if (-not $hostEntry) { continue }
    $hostEntry.ToString().Split(' ', [StringSplitOptions]::RemoveEmptyEntries)
}

$resolvedHosts = foreach ($hostEntry in $expandedHosts) {
    $trimmed = $hostEntry.Trim()
    if (-not $trimmed) { continue }
    if ($trimmed.Contains('.')) {
        $trimmed.ToLowerInvariant()
    }
    else {
        "$trimmed.$Domain".ToLowerInvariant()
    }
}

$uniqueHosts = New-Object System.Collections.Generic.List[string]
$seenHosts = New-Object System.Collections.Generic.HashSet[string] ([StringComparer]::OrdinalIgnoreCase)
foreach ($hostEntry in $resolvedHosts) {
    if ($seenHosts.Add($hostEntry)) {
        $uniqueHosts.Add($hostEntry) | Out-Null
    }
}
$resolvedHosts = $uniqueHosts

if ($resolvedHosts.Count -eq 0) {
    throw 'No hosts provided. Supply -Hosts or leave the defaults.'
}

if (-not (Test-Path -LiteralPath $HostsFile)) {
    throw "Hosts file not found at $HostsFile"
}

$beginMarker = '# croniq-devstack hosts (begin)'
$endMarker = '# croniq-devstack hosts (end)'
$mappingLine = "$IpAddress $($resolvedHosts -join ' ')"
$blockLines = @($beginMarker, $mappingLine, $endMarker)

$lines = Get-Content -LiteralPath $HostsFile
$beginIndex = [Array]::IndexOf($lines, $beginMarker)
$endIndex = [Array]::IndexOf($lines, $endMarker)

$updatedLines = New-Object System.Collections.Generic.List[string]
if ($beginIndex -ge 0 -and $endIndex -gt $beginIndex) {
    if ($beginIndex -gt 0) {
        foreach ($line in @($lines[0..($beginIndex - 1)])) {
            $updatedLines.Add([string]$line) | Out-Null
        }
    }
    foreach ($line in @($blockLines)) {
        $updatedLines.Add([string]$line) | Out-Null
    }
    if ($endIndex + 1 -le $lines.Length - 1) {
        foreach ($line in @($lines[($endIndex + 1)..($lines.Length - 1)])) {
            $updatedLines.Add([string]$line) | Out-Null
        }
    }
}
else {
    foreach ($line in @($lines)) {
        $updatedLines.Add([string]$line) | Out-Null
    }
    if ($updatedLines.Count -gt 0 -and $updatedLines[$updatedLines.Count - 1] -ne '') {
        $updatedLines.Add('')
    }
    foreach ($line in @($blockLines)) {
        $updatedLines.Add([string]$line) | Out-Null
    }
}

if (-not $NoBackup) {
    Copy-Item -LiteralPath $HostsFile -Destination "$HostsFile.bak" -Force
    Write-Host "[devstack] Backup written to $HostsFile.bak" -ForegroundColor DarkGray
}

if ($PSCmdlet.ShouldProcess($HostsFile, "Update hosts for $Domain")) {
    Set-Content -LiteralPath $HostsFile -Value $updatedLines -Encoding ascii
}

Write-Host "[devstack] Hosts updated: $mappingLine" -ForegroundColor Cyan
