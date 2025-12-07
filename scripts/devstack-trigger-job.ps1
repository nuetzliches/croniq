param(
    [Parameter(Mandatory = $true)][string]$JobKey,
    [Parameter(Mandatory = $true)][string]$ApiKey,
    [Parameter(Mandatory = $true)][string]$Endpoint,
    [string]$Initiator = "devstack-script"
)

$ErrorActionPreference = 'Stop'

$metadata = @{ initiator = $Initiator }
$body = @{ jobKey = $JobKey; metadata = $metadata }
$json = $body | ConvertTo-Json -Depth 10 -Compress

$response = Invoke-RestMethod -Uri $Endpoint -Method Post -Headers @{
    'Content-Type' = 'application/json'
    'X-Croniq-Key' = $ApiKey
} -Body $json

$response | ConvertTo-Json -Compress
