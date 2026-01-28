param(
    [Parameter(Mandatory = $true)]
    [string]$TenantId,

    [Parameter(Mandatory = $true)]
    [string]$ClientId,

    [string]$Environment,
    [string]$Name,
    [string]$BaseUrl = "http://localhost:5080",
    [string]$AdminApiKey = $env:CRONIQ_API_KEY,
    [string]$Scopes,
    [int]$TtlHours,
    [switch]$SkipClientUpsert,
    [switch]$EmitEnv
)

if (-not $AdminApiKey) {
    throw "Provide an admin API key via -AdminApiKey or set CRONIQ_API_KEY."
}

$scopeList = @()
if ([string]::IsNullOrWhiteSpace($Scopes)) {
    $scopeList = @(
        "work:poll",
        "work:renew",
        "work:ack",
        "work:events",
        "jobs:register",
        "workers:heartbeat",
        "workers:read",
        "runners:heartbeat",
        "runners:read"
    )
}
else {
    $scopeList = $Scopes -split "[,\s]+" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
}

$headers = @{
    "X-Croniq-Key" = $AdminApiKey
    "Content-Type" = "application/json"
}

$baseUri = $BaseUrl.TrimEnd('/')

if (-not $SkipClientUpsert) {
    $clientPayload = @{
        clientId = $ClientId
        name = $Name
        environmentTag = $Environment
        scopes = $scopeList
        isActive = $true
    }

    if ([string]::IsNullOrWhiteSpace($Name)) {
        $clientPayload.Remove("name")
    }
    if ([string]::IsNullOrWhiteSpace($Environment)) {
        $clientPayload.Remove("environmentTag")
    }
    $clientBody = ($clientPayload | ConvertTo-Json -Depth 6)

    Write-Host "Upserting API client '$ClientId' for tenant '$TenantId'..." -ForegroundColor Cyan
    $clientUri = "$baseUri/tenants/$TenantId/api-clients"
    Invoke-RestMethod -Method Post -Uri $clientUri -Headers $headers -Body $clientBody | Out-Null
}

$keyPayload = @{
    clientId = $ClientId
    environmentTag = $Environment
    scopes = $scopeList
}
if ([string]::IsNullOrWhiteSpace($Environment)) {
    $keyPayload.Remove("environmentTag")
}
if ($PSBoundParameters.ContainsKey('TtlHours')) {
    $keyPayload.ttlHours = [int]$TtlHours
}

$keyBody = ($keyPayload | ConvertTo-Json -Depth 6)

Write-Host "Issuing API key for client '$ClientId'..." -ForegroundColor Cyan
$keyUri = "$baseUri/tenants/$TenantId/api-keys"
$issued = Invoke-RestMethod -Method Post -Uri $keyUri -Headers $headers -Body $keyBody

Write-Host "API key issued (KeyId=$($issued.keyId))." -ForegroundColor Green
Write-Warning "Croniq only returns the plaintext secret once. Persist it now."
Write-Host "Secret: $($issued.plaintextSecret)" -ForegroundColor Yellow

if ($EmitEnv) {
    Write-Host ""
    Write-Host "Suggested env vars:" -ForegroundColor Cyan
    Write-Host "CRONIQ_API_KEY=$($issued.plaintextSecret)"
    Write-Host "CRONIQ_RUNNER_ID=$ClientId"
    Write-Host "CRONIQ_TENANT_ID=$TenantId"
    if ($Environment) {
        Write-Host "CRONIQ_ENVIRONMENT=$Environment"
    }
}

return $issued
