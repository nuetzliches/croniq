param(
    [string]$ArtifactsDir = "$PSScriptRoot/../artifacts",
    [string]$SigningDir = "$PSScriptRoot/../infra/signing",
    [string]$CosignPath = "cosign",
    [SecureString]$Password,
    [switch]$InstallCosign,
    [switch]$Force
)

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

function Resolve-Cosign {
    param([string]$Path)
    # Try the provided path, then common local locations.
    $candidates = @($Path)
    $binDir = Join-Path $PSScriptRoot "../bin"
    $candidates += (Join-Path $binDir "cosign.exe")
    $candidates += (Join-Path $binDir "cosign")

    foreach ($candidate in $candidates | Select-Object -Unique) {
        try {
            return Get-Command $candidate -ErrorAction Stop
        } catch {
            continue
        }
    }

    try {
        return Get-Command $Path -ErrorAction Stop
    } catch {
        return $null
    }
}

$cosign = Resolve-Cosign -Path $CosignPath

    if (-not $cosign -and $InstallCosign) {
    $installer = Join-Path $PSScriptRoot "ci/install-supplychain-tool.ps1"
    if (-not (Test-Path $installer)) {
        throw "cosign not found and installer script missing at $installer. Install manually or update -CosignPath."
    }

    Write-Host "cosign not found. Installing via $installer ..."
    & $installer -Tool cosign

    # If the caller didn't override CosignPath, prefer the freshly installed binary under ./bin
    $defaultBin = Join-Path $PSScriptRoot "../bin/cosign.exe"
    if (-not (Test-Path $defaultBin)) {
        $defaultBin = Join-Path $PSScriptRoot "../bin/cosign"
    }

    if (-not $PSBoundParameters.ContainsKey("CosignPath") -and (Test-Path $defaultBin)) {
        $CosignPath = $defaultBin
    }

    $cosign = Resolve-Cosign -Path $CosignPath
}

if (-not $cosign) {
    throw "cosign was not found at '$CosignPath'. Install it or adjust -CosignPath (use -InstallCosign to auto-install)."
}

$artifactsDir = Resolve-OrCreateDirectory -Path $ArtifactsDir
$signingDir = Resolve-OrCreateDirectory -Path $SigningDir

$keyPath = Join-Path $artifactsDir "cosign.key"
$pubPath = Join-Path $signingDir "cosign.pub"

if (-not $Force) {
    if (Test-Path $keyPath) { throw "Key file already exists at $keyPath. Use -Force to overwrite." }
    if (Test-Path $pubPath) { throw "Public key already exists at $pubPath. Use -Force to overwrite." }
}

# Prompt for password if not provided; allow explicit no-password.
$plainPassword = $null
if (-not $PSBoundParameters.ContainsKey("Password")) {
    $Password = Read-Host "Enter cosign key password (leave blank for none)" -AsSecureString
}

$plainPassword = [Runtime.InteropServices.Marshal]::PtrToStringAuto(
    [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Password)
)

$previousPassword = $env:COSIGN_PASSWORD
$env:COSIGN_PASSWORD = $plainPassword

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("cosign-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

try {
    Push-Location $tempDir
    & $cosign.Source "generate-key-pair"
    Pop-Location

    $tempKey = Join-Path $tempDir "cosign.key"
    $tempPub = Join-Path $tempDir "cosign.pub"

    if (-not (Test-Path $tempKey) -or -not (Test-Path $tempPub)) {
        throw "cosign did not create expected files in $tempDir"
    }

    Copy-Item -Path $tempKey -Destination $keyPath -Force
    Copy-Item -Path $tempPub -Destination $pubPath -Force

    Write-Host "cosign key pair generated."
    Write-Host "Private key: $keyPath"
    Write-Host "Public key : $pubPath (commit this file; keep the private key secret)"
}
finally {
    Pop-Location -ErrorAction SilentlyContinue
    $env:COSIGN_PASSWORD = $previousPassword
    Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
}
