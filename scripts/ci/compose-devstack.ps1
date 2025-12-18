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

function Get-ComposeServiceContainerId {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Service
    )

    $cid = & docker compose @composeFiles @profileArgs ps -q $Service
    if ([string]::IsNullOrWhiteSpace($cid)) {
        $cid = & docker compose @composeFiles @profileArgs ps -a -q $Service
    }

    return ($cid | Select-Object -First 1)
}

function Wait-ForServiceExit {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Service,

        [TimeSpan] $Timeout = ([TimeSpan]::FromMinutes(10)),

        [int] $PollSeconds = 2
    )

    $deadline = (Get-Date).Add($Timeout)
    $cid = Get-ComposeServiceContainerId -Service $Service
    if ([string]::IsNullOrWhiteSpace($cid)) {
        throw "Could not determine container id for service '$Service'."
    }

    while ((Get-Date) -lt $deadline) {
        $state = & docker inspect -f "{{.State.Status}} {{.State.ExitCode}}" $cid
        if (-not [string]::IsNullOrWhiteSpace($state)) {
            $parts = $state -split ' ', 2
            $status = $parts[0]
            $exitCode = 0
            if ($parts.Count -gt 1) { [void][int]::TryParse($parts[1], [ref]$exitCode) }

            if ($status -eq 'exited') {
                return $exitCode
            }
        }

        Start-Sleep -Seconds $PollSeconds
    }

    throw "Timed out waiting for service '$Service' to exit after $($Timeout.TotalMinutes) minutes."
}

switch ($Action) {
    "Up" {
        Invoke-Compose -AdditionalArgs @("up", "--build", "-d")
        $composeExit = $LASTEXITCODE
        if ($composeExit -ne 0) {
            Capture-ComposeDiagnostics -Reason "docker compose up failed" -ExitCode $composeExit
            throw "docker compose up failed with exit code $composeExit. Diagnostics written to '$OutputDirectory'."
        }

        # Some docker compose versions return exit code 0 even if a one-shot service exits non-zero.
        # The devstack requires the migrator to complete successfully before api/worker are considered ready.
        $migratorExit = Wait-ForServiceExit -Service 'croniq-db-migrator' -Timeout ([TimeSpan]::FromMinutes(10))
        if ($migratorExit -ne 0) {
            Capture-ComposeDiagnostics -Reason "croniq-db-migrator failed" -ExitCode $migratorExit
            throw "croniq-db-migrator didn't complete successfully (exit $migratorExit). Diagnostics written to '$OutputDirectory'."
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
