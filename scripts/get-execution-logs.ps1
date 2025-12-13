param(
    [Parameter(Mandatory = $true)]
    [string] $ExecutionId,
    [string] $Endpoint = "http://localhost:5080",
    [string] $ApiKey = $env:CRONIQ_API_KEY,
    [string] $OutputPath
)

if (-not $ApiKey) {
    Write-Error "Provide -ApiKey or set CRONIQ_API_KEY."
    exit 1
}

$uri = "$Endpoint/executions/$ExecutionId/logs"
$headers = @{ "X-Croniq-Key" = $ApiKey }

try {
    if ($OutputPath) {
        Invoke-WebRequest -Uri $uri -Headers $headers -OutFile $OutputPath -ErrorAction Stop
        Write-Host "Logs saved to $OutputPath"
    }
    else {
        $response = Invoke-WebRequest -Uri $uri -Headers $headers -ErrorAction Stop
        $response.Content
    }
}
catch {
    Write-Error $_.Exception.Message
    exit 1
}
