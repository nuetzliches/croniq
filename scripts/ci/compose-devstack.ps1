param(
    [ValidateSet("Up", "Down", "Logs")]
    [string] $Action = "Up",

    [string[]] $Profiles = @("api", "worker", "obs"),

    [string] $OutputDirectory = "artifacts/ci-compose",

    [switch] $CaptureLogs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$composeFiles = @(
    "-f", "infra/docker/docker-compose.yml",
    "-f", "infra/docker/docker-compose.dev.yml",
    "-f", "infra/docker/docker-compose.observability.yml"
)

$profileArgs = @()
foreach ($profile in $Profiles) {
    if (-not [string]::IsNullOrWhiteSpace($profile)) {
        $profileArgs += @("--profile", $profile)
    }
}

$outputPath = Join-Path -Path (Get-Location) -ChildPath $OutputDirectory
New-Item -Path $outputPath -ItemType Directory -Force | Out-Null

function Invoke-Compose {
    param([string[]] $AdditionalArgs)
    & docker compose @composeFiles @profileArgs @AdditionalArgs
}

function Capture-ComposeDiagnostics {
    param(
        [string] $Reason,
        [int] $ExitCode = -1
    )

    try {
        "[{0:O}] compose-devstack diagnostics: {1} (exit={2})" -f (Get-Date), $Reason, $ExitCode | Out-File -FilePath (Join-Path $outputPath "diagnostics.txt") -Encoding utf8 -Append

        & docker compose @composeFiles @profileArgs ps -a *> (Join-Path $outputPath "ps.txt")
        & docker compose @composeFiles @profileArgs config *> (Join-Path $outputPath "compose.config.yml")
        & docker compose @composeFiles @profileArgs logs --no-color *> (Join-Path $outputPath "compose.log")
        & docker compose @composeFiles @profileArgs logs --no-color --tail=500 croniq-db-migrator *> (Join-Path $outputPath "migrator.log")
    }
    catch {
        "[{0:O}] failed to capture compose diagnostics: {1}" -f (Get-Date), $_ | Out-File -FilePath (Join-Path $outputPath "diagnostics.txt") -Encoding utf8 -Append
    }
}

switch ($Action) {
    "Up" {
        Invoke-Compose -AdditionalArgs @("up", "--build", "-d")
        if ($LASTEXITCODE -ne 0) {
            Capture-ComposeDiagnostics -Reason "docker compose up failed" -ExitCode $LASTEXITCODE
            throw "docker compose up failed with exit code $LASTEXITCODE. Diagnostics written to '$OutputDirectory'."
        }
        break
    }
    "Down" {
        if ($CaptureLogs) {
            Capture-ComposeDiagnostics -Reason "capture logs before down"
        }

        Invoke-Compose -AdditionalArgs @("down", "--remove-orphans")
        break
    }
    "Logs" {
        Capture-ComposeDiagnostics -Reason "logs requested"
        break
    }
}
