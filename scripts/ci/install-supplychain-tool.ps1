[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("syft", "trivy")]
    [string]$Tool,

    [string]$Version,

    [string]$InstallDir = "bin"
)

$ErrorActionPreference = 'Stop'

function Get-PlatformInfo {
    $os = $null
    $arch = $null

    try {
        $runtimeType = [System.Runtime.InteropServices.RuntimeInformation]
        if ($runtimeType::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
            $os = 'windows'
        } elseif ($runtimeType::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Linux)) {
            $os = 'linux'
        } elseif ($runtimeType::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::OSX)) {
            $os = 'darwin'
        }

        $archEnum = $runtimeType::ProcessArchitecture
        if ($archEnum) {
            $archValue = $archEnum.ToString()
            $arch = switch ($archValue) {
                'X64' { 'amd64' }
                'Arm64' { 'arm64' }
                Default { $null }
            }
            if (-not $arch) {
                throw "Unsupported CPU architecture: $archValue"
            }
        }
    } catch {
        # RuntimeInformation is not available on downlevel PowerShell; fall back to env heuristics.
    }

    if (-not $os) {
        if ($IsWindows) {
            $os = 'windows'
        } elseif ($IsLinux) {
            $os = 'linux'
        } elseif ($IsMacOS) {
            $os = 'darwin'
        } else {
            throw "Unsupported operating system"
        }
    }

    if (-not $arch) {
        $archHint = $env:PROCESSOR_ARCHITECTURE
        if (-not $archHint -and $env:PROCESSOR_ARCHITEW6432) {
            $archHint = $env:PROCESSOR_ARCHITEW6432
        }
        if (-not $archHint -and $os -ne 'windows') {
            try {
                $archHint = (& uname -m 2>$null).Trim()
            } catch {
                $archHint = $null
            }
        }

        $normalized = if ($archHint) { $archHint.ToLowerInvariant() } else { '' }
        switch -Regex ($normalized) {
            '^(amd64|x86_64)$' { $arch = 'amd64' }
            '^(arm64|aarch64)$' { $arch = 'arm64' }
        }

        if (-not $arch) {
            if ([System.Environment]::Is64BitProcess) {
                $arch = 'amd64'
            } else {
                $hint = if ($archHint) { $archHint } else { 'unknown' }
                throw "Unsupported CPU architecture: $hint"
            }
        }
    }

    return [PSCustomObject]@{
        Os   = $os
        Arch = $arch
    }
}

function Get-SyftAssetInfo {
    param(
        [string]$Version,
        [pscustomobject]$Platform
    )

    $assetOs = switch ($Platform.Os) {
        'windows' { 'windows' }
        'linux' { 'linux' }
        'darwin' { 'darwin' }
        Default { throw "Syft is not packaged for OS '$($Platform.Os)'" }
    }

    $extension = if ($Platform.Os -eq 'windows') { 'zip' } else { 'tar.gz' }
    $fileName = "syft_{0}_{1}_{2}.{3}" -f $Version, $assetOs, $Platform.Arch, $extension
    $uri = "https://github.com/anchore/syft/releases/download/v$Version/$fileName"
    $binaryName = if ($Platform.Os -eq 'windows') { 'syft.exe' } else { 'syft' }

    return [PSCustomObject]@{
        FileName   = $fileName
        Uri        = $uri
        BinaryName = $binaryName
        Archive    = if ($extension -eq 'zip') { 'zip' } else { 'tar' }
    }
}

function Get-TrivyAssetInfo {
    param(
        [string]$Version,
        [pscustomobject]$Platform
    )

    $key = "{0}-{1}" -f $Platform.Os, $Platform.Arch
    $assetMap = @{
        'linux-amd64'   = @{ Name = 'Linux-64bit'; Extension = 'tar.gz' }
        'linux-arm64'   = @{ Name = 'Linux-ARM64'; Extension = 'tar.gz' }
        'darwin-amd64'  = @{ Name = 'macOS-64bit'; Extension = 'tar.gz' }
        'darwin-arm64'  = @{ Name = 'macOS-ARM64'; Extension = 'tar.gz' }
        'windows-amd64' = @{ Name = 'windows-64bit'; Extension = 'zip' }
    }

    if (-not $assetMap.ContainsKey($key)) {
        throw "Trivy does not publish binaries for '$key' yet"
    }

    $selected = $assetMap[$key]
    $fileName = "trivy_{0}_{1}.{2}" -f $Version, $selected.Name, $selected.Extension
    $uri = "https://github.com/aquasecurity/trivy/releases/download/v$Version/$fileName"
    $binaryName = if ($Platform.Os -eq 'windows') { 'trivy.exe' } else { 'trivy' }

    return [PSCustomObject]@{
        FileName   = $fileName
        Uri        = $uri
        BinaryName = $binaryName
        Archive    = if ($selected.Extension -eq 'zip') { 'zip' } else { 'tar' }
    }
}

function Resolve-InstallDir {
    param([string]$Path)
    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }

    $full = Join-Path -Path (Get-Location) -ChildPath $Path
    return [System.IO.Path]::GetFullPath($full)
}

function Install-Tool {
    param(
        [string]$Tool,
        [string]$Version,
        [pscustomobject]$AssetInfo,
        [pscustomobject]$Platform,
        [string]$InstallDir
    )

    $tempDir = Join-Path -Path ([System.IO.Path]::GetTempPath()) -ChildPath ("croniq-" + [System.Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tempDir | Out-Null

    try {
        $downloadPath = Join-Path -Path $tempDir -ChildPath $AssetInfo.FileName
        Write-Host "Downloading $Tool $Version from $($AssetInfo.Uri)"
        Invoke-WebRequest -Uri $AssetInfo.Uri -OutFile $downloadPath -UseBasicParsing

        if ($AssetInfo.Archive -eq 'zip') {
            Expand-Archive -Path $downloadPath -DestinationPath $tempDir -Force
        } else {
            & tar -xzf $downloadPath -C $tempDir
        }

        $binary = Get-ChildItem -Path $tempDir -Recurse -File -Filter $AssetInfo.BinaryName | Select-Object -First 1
        if (-not $binary) {
            throw "Failed to locate $($AssetInfo.BinaryName) in archive"
        }

        $destination = Join-Path -Path $InstallDir -ChildPath $AssetInfo.BinaryName
        Copy-Item -Path $binary.FullName -Destination $destination -Force

        if ($Platform.Os -ne 'windows') {
            & chmod +x -- $destination 2>$null
        }

        Write-Host "Installed $Tool $Version to $destination"
    }
    finally {
        Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$platformInfo = Get-PlatformInfo
$resolvedInstallDir = Resolve-InstallDir -Path $InstallDir
New-Item -ItemType Directory -Path $resolvedInstallDir -Force | Out-Null

if (-not $Version) {
    $repoRoot = (Resolve-Path ([System.IO.Path]::Combine($PSScriptRoot, '..', '..'))).Path
    $versionFile = Join-Path -Path $repoRoot -ChildPath 'eng/versions/supplychain-tools.json'
    if (-not (Test-Path $versionFile)) {
        throw "Missing version manifest at $versionFile"
    }

    $manifest = Get-Content -Path $versionFile -Raw | ConvertFrom-Json
    $Version = $manifest.$Tool.version
    if (-not $Version) {
        throw "No version entry for '$Tool' in $versionFile"
    }
}

$assetInfo = switch ($Tool) {
    'syft' { Get-SyftAssetInfo -Version $Version -Platform $platformInfo }
    'trivy' { Get-TrivyAssetInfo -Version $Version -Platform $platformInfo }
}

Install-Tool -Tool $Tool -Version $Version -AssetInfo $assetInfo -Platform $platformInfo -InstallDir $resolvedInstallDir
