param(
    [Parameter(Mandatory = $false)]
    [string]$ApiBaseUrl = "http://localhost:5080",

    [Parameter(Mandatory = $true)]
    [string]$TenantId,

    [Parameter(Mandatory = $true)]
    [string]$ClientId,

    [Parameter(Mandatory = $true)]
    [string]$SessionToken,

    [Parameter(Mandatory = $false)]
    [string]$EnvironmentTag,

    [Parameter(Mandatory = $false)]
    [string[]]$Scopes,

    [Parameter(Mandatory = $false)]
    [string]$Audience,

    [Parameter(Mandatory = $false)]
    [Nullable[double]]$TtlHours,

    [Parameter(Mandatory = $false)]
    [switch]$Raw
)

$ErrorActionPreference = "Stop"

$baseUrl = $ApiBaseUrl.TrimEnd('/')
$uri = "$baseUrl/tenants/$TenantId/tokens"
if ($EnvironmentTag) {
    $escapedEnvironment = [System.Uri]::EscapeDataString($EnvironmentTag)
    $uri = "$uri?environment=$escapedEnvironment"
}

$ttlMinutes = $null
if ($TtlHours -ne $null) {
    # Backend typically expects integer minutes; round to the nearest minute.
    $ttlMinutes = [int][Math]::Round($TtlHours.Value * 60)
}

$bodyObject = @{
    clientId = $ClientId
    scopes = if ($Scopes -and $Scopes.Length -gt 0) { $Scopes } else { $null }
    audience = if ($Audience) { $Audience } else { $null }
    ttlMinutes = $ttlMinutes
}

$headers = @{
    Authorization = "Bearer $SessionToken"
}

Write-Host "POST $uri" -ForegroundColor Cyan
Write-Host "Request body:" -ForegroundColor Cyan
$bodyJson = ($bodyObject | ConvertTo-Json -Depth 10)
Write-Host $bodyJson

$response = Invoke-RestMethod -Method Post -Uri $uri -Headers $headers -ContentType "application/json" -Body $bodyJson

if ($Raw) {
    $response
    exit 0
}

Write-Host "\nResponse (JSON):" -ForegroundColor Green
$response | ConvertTo-Json -Depth 20
