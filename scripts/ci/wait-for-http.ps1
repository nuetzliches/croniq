param(
    [Parameter(Mandatory = $true)]
    [string] $Uri,

    [int] $Attempts = 60,
    [int] $DelaySeconds = 2,
    [int] $TimeoutSeconds = 5
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

for ($i = 0; $i -lt $Attempts; $i++) {
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $Uri -TimeoutSec $TimeoutSeconds | Out-Null
        Write-Host "Endpoint healthy: $Uri"
        exit 0
    } catch {
        Start-Sleep -Seconds $DelaySeconds
    }
}

Write-Error "Endpoint did not become healthy: $Uri"
exit 1
