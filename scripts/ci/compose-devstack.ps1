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

switch ($Action) {
    "Up" {
        Invoke-Compose -AdditionalArgs @("up", "--build", "-d")
        break
    }
    "Down" {
        Invoke-Compose -AdditionalArgs @("down", "--remove-orphans")
        if ($CaptureLogs) {
            & docker compose @composeFiles @profileArgs logs *> (Join-Path $outputPath "compose.log")
        }
        break
    }
    "Logs" {
        & docker compose @composeFiles @profileArgs logs *> (Join-Path $outputPath "compose.log")
        break
    }
}
