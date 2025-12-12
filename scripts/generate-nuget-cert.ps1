param(
    [string]$Subject = "CN=Croniq NuGet Signing",
    [int]$ValidityYears = 1,
    [string]$ArtifactsDir = "$PSScriptRoot/../artifacts",
    [string]$SigningDir = "$PSScriptRoot/../infra/signing",
    [SecureString]$Password,
    [switch]$EmitBase64 = $true,
    [string]$Base64Path = "$PSScriptRoot/../artifacts/nuget-signing.pfx.b64",
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

function Resolve-OrCreateDirectory {
    param([string]$Path)
    $resolved = Resolve-Path -Path $Path -ErrorAction SilentlyContinue
    if (-not $resolved) {
        $resolved = (New-Item -ItemType Directory -Path $Path -Force).FullName
    } else {
        $resolved = $resolved.ProviderPath
    }
    return $resolved
}

$artifactsDir = Resolve-OrCreateDirectory -Path $ArtifactsDir
$signingDir = Resolve-OrCreateDirectory -Path $SigningDir

$pfxPath = Join-Path $artifactsDir "nuget-signing.pfx"
$cerPath = Join-Path $signingDir "nuget-signing.cer"

if (-not $Force) {
    if (Test-Path $pfxPath) { throw "PFX already exists at $pfxPath. Use -Force to overwrite." }
    if (Test-Path $cerPath) { throw "CER already exists at $cerPath. Use -Force to overwrite." }
    if ($EmitBase64 -and (Test-Path $Base64Path)) { throw "Base64 file already exists at $Base64Path. Use -Force to overwrite." }
}

if (-not $PSBoundParameters.ContainsKey("Password")) {
    $Password = Read-Host "Enter password for the NuGet signing PFX (leave blank for none)" -AsSecureString
}

Write-Host "Creating code signing certificate: $Subject (valid $ValidityYears year(s))"
$cert = New-SelfSignedCertificate `
    -Type CodeSigning `
    -Subject $Subject `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -NotAfter (Get-Date).AddYears([Math]::Max(1, $ValidityYears))

if (-not $cert) {
    throw "Failed to create the certificate."
}

Write-Host "Exporting public certificate to $cerPath"
Export-Certificate -Cert $cert -FilePath $cerPath | Out-Null

Write-Host "Exporting PFX to $pfxPath"
Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $Password | Out-Null

if ($EmitBase64) {
    $pfxBytes = [System.IO.File]::ReadAllBytes($pfxPath)
    $encoded = [Convert]::ToBase64String($pfxBytes)

    $resolvedBase64Path = $Base64Path
    $maybeResolved = Resolve-Path -Path $Base64Path -ErrorAction SilentlyContinue
    if ($maybeResolved) {
        $resolvedBase64Path = $maybeResolved.ProviderPath
    }

    $base64Dir = Split-Path -Path $resolvedBase64Path -Parent
    Resolve-OrCreateDirectory -Path $base64Dir | Out-Null
    [System.IO.File]::WriteAllText($resolvedBase64Path, $encoded)
    Write-Host "Base64-encoded PFX written to $resolvedBase64Path (keep secret)."
}

Write-Host "NuGet signing certificate created."
Write-Host "Public CER: $cerPath (commit this)"
Write-Host "Private PFX: $pfxPath (keep secret; encode as base64 for CI secret)."
Write-Host "Thumbprint: $($cert.Thumbprint)"
