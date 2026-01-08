param(
    [string]$TenantId = "default",
    [string]$EnvironmentTag = "dev",
    [string]$DmzApiKey = "dmz-sample-key",
    [string]$InternalApiKey = "dmz-sample-key",
    [string]$DmzHttpUrl = "http://localhost:5000",
    [string]$DmzGrpcUrl = "https://localhost:5001",
    [string]$InternalUrl = "http://localhost:5080",
    [string]$DmzSqlPort = "11434",
    [string]$InternalSqlPort = "11433",
    [string]$DmzDatabase = "CroniqDmz",
    [string]$InternalDatabase = "CroniqDev",
    [string]$SqlPassword = "CroniqSqlP@ssw0rd!",
    [string]$WebhookSecret = "dmz-webhook-secret",
    [string]$WebhookHookKey = "invoice-paid",
    [string]$WebhookPayload = '{"invoiceId":"INV-42"}',
    [switch]$SkipDocker,
    [switch]$SkipCleanup,
    [switch]$KeepExistingHosts
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$logsDir = Join-Path $repoRoot "logs"
New-Item -ItemType Directory -Path $logsDir -Force | Out-Null

$dmzLog = Join-Path $logsDir "smoke-dmz-dmz.log"
$dmzErrorLog = Join-Path $logsDir "smoke-dmz-dmz.err.log"
$internalLog = Join-Path $logsDir "smoke-dmz-internal.log"
$internalErrorLog = Join-Path $logsDir "smoke-dmz-internal.err.log"

$dmzSqlContainer = "croniq-sql-dmz"
$internalSqlContainer = "croniq-sql-internal"

if ([string]::IsNullOrWhiteSpace($DmzSqlPort)) {
    $DmzSqlPort = "11434"
}

if ([string]::IsNullOrWhiteSpace($InternalSqlPort)) {
    $InternalSqlPort = "11433"
}

$dmzConnectionString = "Server=localhost,$DmzSqlPort;Database=$DmzDatabase;User Id=sa;Password=$SqlPassword;Encrypt=True;TrustServerCertificate=True;"
$internalConnectionString = "Server=localhost,$InternalSqlPort;Database=$InternalDatabase;User Id=sa;Password=$SqlPassword;Encrypt=True;TrustServerCertificate=True;"

$dmzProcess = $null
$internalProcess = $null
$createdDmzSql = $false
$createdInternalSql = $false
$startedDmzSql = $false
$startedInternalSql = $false

function Write-Step([string]$message) {
    Write-Host $message -ForegroundColor Cyan
}

function Wait-ForPort([string]$hostName, [int]$port, [int]$timeoutSeconds) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $result = Test-NetConnection -ComputerName $hostName -Port $port -WarningAction SilentlyContinue
        if ($result.TcpTestSucceeded) {
            return
        }
        Start-Sleep -Seconds 2
    }
    throw "Timed out waiting for ${hostName}:$port."
}

function Wait-ForHttp([string]$url, [int]$timeoutSeconds) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            Invoke-WebRequest -Method Get -Uri $url -UseBasicParsing -TimeoutSec 5 | Out-Null
            return
        }
        catch {
            Start-Sleep -Seconds 2
        }
    }
    throw "Timed out waiting for $url."
}

function New-CroniqWebhookSignature([string]$secret, [string]$payload) {
    $hmac = [System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($secret))
    try {
        $hash = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($payload))
    }
    finally {
        $hmac.Dispose()
    }
    $hex = ($hash | ForEach-Object { $_.ToString("x2") }) -join ""
    return "sha256=$hex"
}

function Invoke-Docker([string[]]$dockerArgs) {
    $process = Start-Process -FilePath "docker" -ArgumentList $dockerArgs -NoNewWindow -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "Docker command failed."
    }
}

function Ensure-SqlContainer([string]$name, [string]$sqlPort, [ref]$created, [ref]$started) {
    $existing = docker ps -a --format "{{.Names}}" | Where-Object { $_ -eq $name }
    if (-not $existing) {
        $portCheck = Test-NetConnection -ComputerName "localhost" -Port ([int]$sqlPort) -WarningAction SilentlyContinue
        if ($portCheck.TcpTestSucceeded) {
            Write-Step "Port $sqlPort is already in use; skipping container start."
            return
        }

        Write-Step "Starting SQL container $name on port $sqlPort..."
        $dockerArgs = @(
            "run",
            "-d",
            "--name",
            $name,
            "-e",
            "ACCEPT_EULA=Y",
            "-e",
            "MSSQL_SA_PASSWORD=$SqlPassword",
            "-e",
            "MSSQL_PID=Developer",
            "-p",
            "$($sqlPort):1433",
            "mcr.microsoft.com/mssql/server:2022-latest"
        )
        for ($i = 0; $i -lt $dockerArgs.Count; $i++) {
            if ([string]::IsNullOrEmpty($dockerArgs[$i])) {
                throw "Docker args contained an empty value at index $i."
            }
        }
        Invoke-Docker -dockerArgs $dockerArgs
        $created.Value = $true
        return
    }

    $running = docker ps --format "{{.Names}}" | Where-Object { $_ -eq $name }
    if (-not $running) {
        Write-Step "Starting existing SQL container $name..."
        $portCheck = Test-NetConnection -ComputerName "localhost" -Port ([int]$sqlPort) -WarningAction SilentlyContinue
        if ($portCheck.TcpTestSucceeded) {
            Write-Step "Port $sqlPort is already in use; skipping container start."
        }
        else {
            Invoke-Docker -dockerArgs @("start", $name)
            $started.Value = $true
        }
    }
}

function Invoke-Migrator([string]$connectionString) {
    Write-Step "Applying migrations with Croniq.DbMigrator..."
    $saved = @{
        CRONIQ_SQL_CONNECTION = $env:CRONIQ_SQL_CONNECTION
        CRONIQ_SEED_ADMIN = $env:CRONIQ_SEED_ADMIN
        CRONIQ_SEED_TENANT_ID = $env:CRONIQ_SEED_TENANT_ID
        CRONIQ_SEED_TENANT_NAME = $env:CRONIQ_SEED_TENANT_NAME
        CRONIQ_SEED_TENANT_REFERENCE = $env:CRONIQ_SEED_TENANT_REFERENCE
        CRONIQ_SEED_ADMIN_PASSWORD_CHANGE_REQUIRED = $env:CRONIQ_SEED_ADMIN_PASSWORD_CHANGE_REQUIRED
    }

    try {
        $env:CRONIQ_SQL_CONNECTION = $connectionString
        $env:CRONIQ_SEED_ADMIN = "true"
        $env:CRONIQ_SEED_TENANT_ID = $TenantId
        $env:CRONIQ_SEED_TENANT_NAME = $TenantId
        $env:CRONIQ_SEED_TENANT_REFERENCE = $TenantId
        $env:CRONIQ_SEED_ADMIN_PASSWORD_CHANGE_REQUIRED = "false"

        dotnet run --project (Join-Path $repoRoot "tools/Croniq.DbMigrator/Croniq.DbMigrator.csproj")
        if ($LASTEXITCODE -ne 0) {
            throw "Croniq.DbMigrator failed for $connectionString."
        }
    }
    finally {
        foreach ($name in $saved.Keys) {
            if ($null -eq $saved[$name]) {
                Remove-Item "Env:$name" -ErrorAction SilentlyContinue
            }
            else {
                Set-Item -Path "Env:$name" -Value $saved[$name]
            }
        }
    }
}

function Stop-ExistingHost([string]$processName) {
    $processes = Get-Process -Name $processName -ErrorAction SilentlyContinue
    if ($processes) {
        Write-Step "Stopping existing $processName process(es) to avoid file locks."
        $processes | Stop-Process -Force
        Start-Sleep -Seconds 2
    }
}

function Start-Host([string]$projectPath, [string[]]$args, [string]$logPath, [string]$errorLogPath, [hashtable]$environmentOverrides = $null, [switch]$NoBuild) {
    $fullArgs = @("run")
    if ($NoBuild) {
        $fullArgs += "--no-build"
    }
    $fullArgs += @("--project", $projectPath, "--") + $args
    $startArgs = @{
        FilePath = "dotnet"
        ArgumentList = $fullArgs
        WorkingDirectory = $repoRoot
        NoNewWindow = $true
        PassThru = $true
    }
    if (-not [string]::IsNullOrWhiteSpace($logPath)) {
        $startArgs.RedirectStandardOutput = $logPath
    }
    if (-not [string]::IsNullOrWhiteSpace($errorLogPath)) {
        $startArgs.RedirectStandardError = $errorLogPath
    }

    $saved = @{}
    if ($null -ne $environmentOverrides) {
        foreach ($entry in $environmentOverrides.GetEnumerator()) {
            $saved[$entry.Key] = [Environment]::GetEnvironmentVariable($entry.Key)
            [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value)
        }
    }

    try {
        return Start-Process @startArgs
    }
    finally {
        foreach ($name in $saved.Keys) {
            [Environment]::SetEnvironmentVariable($name, $saved[$name])
        }
    }
}

function Wait-ForExecution([string]$baseUrl, [string]$apiKey, [string]$jobKey, [int]$timeoutSeconds) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    $encodedJobKey = [Uri]::EscapeDataString($jobKey)
    $encodedEnvironment = [Uri]::EscapeDataString($EnvironmentTag)
    $requestUrl = "$baseUrl/tenants/$TenantId/executions?jobKey=$encodedJobKey&environment=$encodedEnvironment&limit=5"
    $headers = @{ "X-Croniq-Key" = $apiKey }

    while ((Get-Date) -lt $deadline) {
        try {
            $response = Invoke-RestMethod -Method Get -Uri $requestUrl -Headers $headers -TimeoutSec 5
            if ($null -ne $response) {
                if ($response -is [System.Array]) {
                    foreach ($item in $response) {
                        if ($item.jobKey -eq $jobKey) {
                            return $item
                        }
                    }
                }
                elseif ($response.jobKey -eq $jobKey) {
                    return $response
                }
            }
        }
        catch {
            Start-Sleep -Seconds 2
        }
        Start-Sleep -Seconds 2
    }

    throw "Timed out waiting for execution of '$jobKey'."
}

try {
    if (-not $KeepExistingHosts) {
        Stop-ExistingHost -processName "Croniq.Sample.ApiHost"
        Stop-ExistingHost -processName "Croniq.Sample.Dmz"
    }

    if (-not $SkipDocker) {
        if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
            throw "Docker is required. Install Docker Desktop or use -SkipDocker with pre-started SQL servers."
        }

        Ensure-SqlContainer -name $dmzSqlContainer -sqlPort $DmzSqlPort -created ([ref]$createdDmzSql) -started ([ref]$startedDmzSql)
        Ensure-SqlContainer -name $internalSqlContainer -sqlPort $InternalSqlPort -created ([ref]$createdInternalSql) -started ([ref]$startedInternalSql)

        Write-Step "Waiting for SQL ports..."
        Wait-ForPort -hostName "localhost" -port ([int]$DmzSqlPort) -timeoutSeconds 90
        Wait-ForPort -hostName "localhost" -port ([int]$InternalSqlPort) -timeoutSeconds 90
    }

    Invoke-Migrator -connectionString $dmzConnectionString
    Invoke-Migrator -connectionString $internalConnectionString

    $env:DOTNET_ENVIRONMENT = "Development"
    $env:ASPNETCORE_ENVIRONMENT = "Development"

    Write-Step "Building sample hosts..."
    dotnet build (Join-Path $repoRoot "samples/Croniq.Sample.Dmz/Croniq.Sample.Dmz.csproj")
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to build Croniq.Sample.Dmz."
    }
    dotnet build (Join-Path $repoRoot "samples/Croniq.Sample.ApiHost/Croniq.Sample.ApiHost.csproj")
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to build Croniq.Sample.ApiHost."
    }

    Write-Step "Starting DMZ host..."
    $dmzEnv = @{
        "Croniq__Auth__Mode" = "InMemory"
        "Croniq__Auth__InMemory__ApiKey" = $DmzApiKey
        "Croniq__Auth__InMemory__TenantId" = $TenantId
        "Croniq__Auth__InMemory__EnvironmentTag" = $EnvironmentTag
        "Croniq__SqlServer__ConnectionString" = $dmzConnectionString
        "ASPNETCORE_URLS" = "$DmzGrpcUrl;$DmzHttpUrl"
    }
    $dmzArgs = @(
        "--Croniq:Auth:Mode=InMemory",
        "--Croniq:Auth:InMemory:ApiKey=$DmzApiKey",
        "--Croniq:Auth:InMemory:TenantId=$TenantId",
        "--Croniq:Auth:InMemory:EnvironmentTag=$EnvironmentTag",
        "--Croniq:SqlServer:ConnectionString=$dmzConnectionString"
    )
    $dmzProcess = Start-Host -projectPath "samples/Croniq.Sample.Dmz/Croniq.Sample.Dmz.csproj" -args $dmzArgs -logPath $dmzLog -errorLogPath $dmzErrorLog -environmentOverrides $dmzEnv -NoBuild

    Write-Step "Starting internal API host..."
    $internalEnv = @{
        "Croniq__Auth__Mode" = "InMemory"
        "Croniq__Auth__InMemory__ApiKey" = $InternalApiKey
        "Croniq__Auth__InMemory__TenantId" = $TenantId
        "Croniq__Auth__InMemory__EnvironmentTag" = $EnvironmentTag
        "Croniq__Webhooks__Mode" = "Remote"
        "Croniq__Webhooks__Remote__BaseUrl" = $DmzGrpcUrl
        "Croniq__Webhooks__Remote__ApiKey" = $DmzApiKey
        "Croniq__Webhooks__Remote__EnableRelay" = "true"
        "Croniq__Webhooks__Remote__StreamMode" = "Grpc"
        "Croniq__SqlServer__ConnectionString" = $internalConnectionString
        "ASPNETCORE_URLS" = $InternalUrl
    }
    $internalArgs = @(
        "--urls", $InternalUrl,
        "--Croniq:Auth:Mode=InMemory",
        "--Croniq:Auth:InMemory:ApiKey=$InternalApiKey",
        "--Croniq:Auth:InMemory:TenantId=$TenantId",
        "--Croniq:Auth:InMemory:EnvironmentTag=$EnvironmentTag",
        "--Croniq:Webhooks:Mode=Remote",
        "--Croniq:Webhooks:Remote:BaseUrl=$DmzGrpcUrl",
        "--Croniq:Webhooks:Remote:ApiKey=$DmzApiKey",
        "--Croniq:Webhooks:Remote:EnableRelay=true",
        "--Croniq:Webhooks:Remote:StreamMode=Grpc",
        "--Croniq:SqlServer:ConnectionString=$internalConnectionString"
    )
    $internalProcess = Start-Host -projectPath "samples/Croniq.Sample.ApiHost/Croniq.Sample.ApiHost.csproj" -args $internalArgs -logPath $internalLog -errorLogPath $internalErrorLog -environmentOverrides $internalEnv -NoBuild

    Write-Step "Waiting for DMZ and internal hosts..."
    Wait-ForHttp -url "$DmzHttpUrl/health" -timeoutSeconds 120
    Wait-ForHttp -url "$InternalUrl/health" -timeoutSeconds 120

    Write-Step "Sending webhook to DMZ..."
    $signature = New-CroniqWebhookSignature -secret $WebhookSecret -payload $WebhookPayload
    $webhookUrl = "$DmzHttpUrl/tenants/$TenantId/environments/$EnvironmentTag/webhooks/$WebhookHookKey"
    Invoke-RestMethod -Method Post -Uri $webhookUrl -ContentType "application/json" -Headers @{ "X-Croniq-Signature" = $signature } -Body $WebhookPayload -TimeoutSec 15 | Out-Null

    Write-Step "Waiting for internal execution log..."
    $execution = Wait-ForExecution -baseUrl $InternalUrl -apiKey $InternalApiKey -jobKey "samples:logging-job" -timeoutSeconds 60
    Write-Host "DMZ relay smoke succeeded (executionId=$($execution.executionId))." -ForegroundColor Green
}
finally {
    if (-not $SkipCleanup) {
        if ($internalProcess -and -not $internalProcess.HasExited) {
            Stop-Process -Id $internalProcess.Id -Force
        }
        if ($dmzProcess -and -not $dmzProcess.HasExited) {
            Stop-Process -Id $dmzProcess.Id -Force
        }

        if (-not $SkipDocker) {
            if ($createdInternalSql) {
                Invoke-Docker -dockerArgs @("rm", "-f", $internalSqlContainer)
            }
            elseif ($startedInternalSql) {
                Invoke-Docker -dockerArgs @("stop", $internalSqlContainer)
            }

            if ($createdDmzSql) {
                Invoke-Docker -dockerArgs @("rm", "-f", $dmzSqlContainer)
            }
            elseif ($startedDmzSql) {
                Invoke-Docker -dockerArgs @("stop", $dmzSqlContainer)
            }
        }
    }
}
