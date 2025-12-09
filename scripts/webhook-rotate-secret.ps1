param(
    [Parameter(Mandatory = $true)]
    [string]$TenantId,

    [Parameter(Mandatory = $true)]
    [string]$Environment,

    [Parameter(Mandatory = $true)]
    [string]$HookKey,

    [int]$ActivateInSeconds,
    [int]$GracePeriodSeconds,
    [string]$Notes,

    [string]$BaseUrl = "http://localhost:5100/",
    [string]$ApiKey = $env:CRONIQ_API_KEY
)

if (-not $ApiKey) {
    throw "Provide an API key via -ApiKey or set the CRONIQ_API_KEY environment variable."
}

$baseUri = $BaseUrl.TrimEnd('/')
$uri = "$baseUri/tenants/$TenantId/webhooks/$HookKey/rotate-secret?environment=$Environment"
$headers = @{
    "X-Croniq-Key" = $ApiKey
    "Content-Type" = "application/json"
}

$payload = @{}
if ($PSBoundParameters.ContainsKey('ActivateInSeconds')) {
    $payload.activateInSeconds = [int]$ActivateInSeconds
}
if ($PSBoundParameters.ContainsKey('GracePeriodSeconds')) {
    $payload.gracePeriodSeconds = [int]$GracePeriodSeconds
}
if ($PSBoundParameters.ContainsKey('Notes') -and ![string]::IsNullOrWhiteSpace($Notes)) {
    $payload.notes = $Notes
}

$bodyJson = if ($payload.Count -eq 0) { '{}' } else { $payload | ConvertTo-Json -Depth 4 }

Write-Host "Rotating secret for webhook '$HookKey' in tenant '$TenantId' ($Environment)..." -ForegroundColor Cyan
$response = Invoke-RestMethod -Method Post -Uri $uri -Headers $headers -Body $bodyJson

$activationInfo = "activates at $($response.activatedAtUtc)"
if ($response.expiresAtUtc) {
    $activationInfo += ", expires at $($response.expiresAtUtc)"
}

Write-Host "Rotation succeeded: $activationInfo" -ForegroundColor Green
Write-Warning "Croniq only returns the plaintext secret once. Persist it in your secret manager now."
Write-Host "Secret: $($response.secret)" -ForegroundColor Yellow

return $response
