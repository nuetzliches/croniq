param(
    [ValidateSet("list", "replay")]
    [string]$Action = "list",
    [Parameter(Mandatory = $true)]
    [string]$TenantId,
    [Parameter(Mandatory = $true)]
    [string]$Environment,
    [string]$BaseUrl = "http://localhost:5100/",
    [string]$ApiKey = $env:CRONIQ_API_KEY,
    [long]$Id
)

if (-not $ApiKey) {
    throw "Provide an API key via -ApiKey or set the CRONIQ_API_KEY environment variable."
}

$baseUri = $BaseUrl.TrimEnd('/')
$headers = @{ "X-Croniq-Key" = $ApiKey }

switch ($Action) {
    "list" {
        $url = "$baseUri/tenants/$TenantId/webhooks/deadletters?environment=$Environment"
        Invoke-RestMethod -Method Get -Uri $url -Headers $headers
    }
    "replay" {
        if (-not $Id) {
            throw "Provide -Id when using Action 'replay'."
        }
        $url = "$baseUri/tenants/$TenantId/webhooks/deadletters/$Id/replay?environment=$Environment"
        Invoke-RestMethod -Method Post -Uri $url -Headers $headers
    }
    default {
        throw "Unsupported action $Action"
    }
}
