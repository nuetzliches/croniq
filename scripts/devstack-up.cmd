@echo off
setlocal enabledelayedexpansion

set ROOT=%~dp0..
cd /d "%ROOT%" || exit /b 1

REM Help / usage
if /I "%~1"=="--help" goto help
if /I "%~1"=="-h" goto help
if /I "%~1"=="/?" goto help

set RUN_SAMPLE=
set NO_SAMPLE=

if not exist ".env" (
    echo [devstack] Missing .env in repo root. Copy .env.example to .env first.
    exit /b 1
)

set COMPOSE_ARGS=-f infra\docker\docker-compose.yml -f infra\docker\docker-compose.dev.yml -f infra\docker\docker-compose.observability.yml
set DEFAULT_PROFILES=--profile worker

if "%CRONIQ_API_HTTP_PORT%"=="" set CRONIQ_API_HTTP_PORT=5080
if "%CRONIQ_API_BASEURL%"=="" set CRONIQ_API_BASEURL=http://localhost:%CRONIQ_API_HTTP_PORT%
set HEALTH_URL=%CRONIQ_API_BASEURL%
if not "%HEALTH_URL:~-1%"=="/" set HEALTH_URL=%HEALTH_URL%/
set HEALTH_URL=%HEALTH_URL%health

REM Parse args: we keep passing through profiles, but optionally run a host sample.
set USER_PROFILES=
:parse
if "%~1"=="" goto parsed
if /I "%~1"=="--help" goto help
if /I "%~1"=="-h" goto help
if /I "%~1"=="/?" goto help
if /I "%~1"=="--no-sample" goto handle_no_sample
if /I "%~1"=="--sample" goto handle_sample
set USER_PROFILES=%USER_PROFILES% %~1
shift
goto parse

:handle_no_sample
set NO_SAMPLE=1
shift
goto parse

:handle_sample
shift
if "%~1"=="" goto parsed
set RUN_SAMPLE=%~1
shift
goto parse

:parsed

REM Avoid duplicate worker profile if the user already passed it explicitly.
echo %USER_PROFILES% | find /I "--profile worker" >nul
if not errorlevel 1 set DEFAULT_PROFILES=

if not "%CRONIQ_DEVSTACK_PROFILES%"=="" (
    set PROFILE_ARGS=%CRONIQ_DEVSTACK_PROFILES%
) else (
    set PROFILE_ARGS=%DEFAULT_PROFILES% %USER_PROFILES%
)

if not "%CRONIQ_DEVSTACK_PROFILES%"=="" (
    echo [devstack] Using CRONIQ_DEVSTACK_PROFILES override: %CRONIQ_DEVSTACK_PROFILES%
)

echo [devstack] USER_PROFILES=%USER_PROFILES%

call :detect_profiles "%PROFILE_ARGS%"
echo [devstack] PROFILE_ARGS=%PROFILE_ARGS% (hasApi=!HAS_API_PROFILE!, hasObs=!HAS_OBS_PROFILE!)

REM Explain whether/why we start the host ApiHost sample.
set AUTO_SAMPLE=0
set SAMPLE_REASON=

if not "%RUN_SAMPLE%"=="" (
    set SAMPLE_REASON=explicit --sample %RUN_SAMPLE%
) else if not "%NO_SAMPLE%"=="" (
    set SAMPLE_REASON=disabled via --no-sample
) else (
    if "!HAS_API_PROFILE!"=="1" (
        set SAMPLE_REASON=disabled because container api profile requested
    ) else (
        set RUN_SAMPLE=apihost
        set AUTO_SAMPLE=1
        set SAMPLE_REASON=enabled by default
    )
)

if "%RUN_SAMPLE%"=="" (
    echo [devstack] Host ApiHost not started: !SAMPLE_REASON!
) else (
    echo [devstack] Host ApiHost will start: !SAMPLE_REASON!
)

REM Guard: do not allow container api and host api simultaneously.
if /I "%RUN_SAMPLE%"=="apihost" (
    if "!HAS_API_PROFILE!"=="1" (
        echo [devstack] Invalid configuration: Host ApiHost --sample apihost cannot be combined with container profile 'api'.
        echo [devstack] Remove '--profile api' or set --no-sample to run container api instead.
        exit /b 1
    )
)

echo [devstack] Starting Croniq services (%PROFILE_ARGS%)...
docker compose %COMPOSE_ARGS% %PROFILE_ARGS% up --build -d
if errorlevel 1 (
    echo [devstack] docker compose up failed.
    exit /b 1
)

call :wait_for_migrator "%COMPOSE_ARGS%"
if errorlevel 1 exit /b 1

call :maybe_wait_for_api "%PROFILE_ARGS%" "%HEALTH_URL%" "%RUN_SAMPLE%"
if errorlevel 1 exit /b 1

call :maybe_start_sample "%RUN_SAMPLE%" "%HEALTH_URL%" "%PROFILE_ARGS%"
if errorlevel 1 exit /b 1

echo [devstack] Croniq dev stack is ready.
exit /b 0

:maybe_start_sample
setlocal
set SAMPLE=%~1
set HEALTH_URL=%~2
set PROFILE_ARGS=%~3
if "%SAMPLE%"=="" (endlocal & exit /b 0)
if /I "%SAMPLE%"=="apihost" goto start_apihost
echo [devstack] Unknown sample '%SAMPLE%'. Supported: apihost
endlocal & exit /b 1

:start_apihost
if "%CRONIQ_SAMPLE_APIHOST_HTTP_PORT%"=="" set CRONIQ_SAMPLE_APIHOST_HTTP_PORT=%CRONIQ_API_HTTP_PORT%

set PID_DIR=artifacts\devstack
if not exist "%PID_DIR%" mkdir "%PID_DIR%" >nul 2>&1
set PID_FILE=%PID_DIR%\sample-apihost.pid
set OUT_LOG=%PID_DIR%\sample-apihost.out.log
set ERR_LOG=%PID_DIR%\sample-apihost.err.log
set START_LOG=%PID_DIR%\sample-apihost.start.log

REM Stop previous instance if pid file exists.
if exist "%PID_FILE%" (
    for /f "usebackq delims=" %%P in ("%PID_FILE%") do set OLD_PID=%%P
    if not "%OLD_PID%"=="" (
        powershell -NoProfile -Command "try { Stop-Process -Id %OLD_PID% -Force -ErrorAction SilentlyContinue } catch {}" >nul 2>&1
    )
    del /q "%PID_FILE%" >nul 2>&1
)

echo [devstack] Starting sample ApiHost via dotnet run on http://localhost:%CRONIQ_SAMPLE_APIHOST_HTTP_PORT% ...
REM Build host connection string in PowerShell to avoid delayed-expansion issues with '!' in passwords.

REM If observability profile is enabled, prefer localhost OTLP endpoint for host-run processes.
set CRONIQ_HOST_OTLP_ENDPOINT=
call :detect_profiles "%PROFILE_ARGS%"
if "!HAS_OBS_PROFILE!"=="1" (
    if "%CRONIQ_OTLP_GRPC_PORT%"=="" set CRONIQ_OTLP_GRPC_PORT=4317
    set CRONIQ_HOST_OTLP_ENDPOINT=http://localhost:%CRONIQ_OTLP_GRPC_PORT%
)

REM Use PowerShell to set env vars + capture PID reliably.
del /q "%OUT_LOG%" >nul 2>&1
del /q "%ERR_LOG%" >nul 2>&1
del /q "%START_LOG%" >nul 2>&1
powershell -NoProfile -Command "$env:ASPNETCORE_URLS='http://0.0.0.0:%CRONIQ_SAMPLE_APIHOST_HTTP_PORT%'; if ($env:CRONIQ_DOTNET_ENVIRONMENT) { $env:DOTNET_ENVIRONMENT=$env:CRONIQ_DOTNET_ENVIRONMENT }; $env:Croniq__Auth__Mode=$env:CRONIQ_AUTH_MODE; $env:Croniq__Auth__InMemory__ApiKey=$env:CRONIQ_SMOKE_API_KEY; $env:Croniq__Auth__InMemory__TenantId=$env:CRONIQ_CORE_TENANT_ID; $env:Croniq__Auth__InMemory__EnvironmentTag=$env:CRONIQ_CORE_ENVIRONMENT; $env:Croniq__Persistence__Mode='SqlServer'; $env:Croniq__Api__RequestsPerMinute=$env:CRONIQ_API_REQUESTS_PER_MINUTE; $env:Croniq__Core__TenantId=$env:CRONIQ_CORE_TENANT_ID; $env:Croniq__Core__EnvironmentTag=$env:CRONIQ_CORE_ENVIRONMENT; $env:Croniq__Core__InstanceId=$env:CRONIQ_API_INSTANCE_ID; $dotenv=@{}; if (Test-Path '.env') { Get-Content '.env' | ForEach-Object { $l=$_.Trim(); if (!$l -or $l.StartsWith('#')) { return }; $parts=$l -split '=',2; if ($parts.Count -eq 2) { $dotenv[$parts[0].Trim()]=$parts[1].Trim() } } }; function Get-EnvOrDotenv([string]$k,[string]$fallback) { $v=[Environment]::GetEnvironmentVariable($k); if ($v) { return $v }; if ($dotenv.ContainsKey($k) -and $dotenv[$k]) { return $dotenv[$k] }; return $fallback }; $sqlPort = Get-EnvOrDotenv 'CRONIQ_SQL_HOST_PORT' '11433'; $sqlDb = Get-EnvOrDotenv 'CRONIQ_SQL_DATABASE' 'CroniqDev'; $sqlPw = Get-EnvOrDotenv 'CRONIQ_SQL_PASSWORD' 'CroniqSqlP@ssw0rd!'; $env:Croniq__SqlServer__ConnectionString = ('Server=localhost,' + $sqlPort + ';Database=' + $sqlDb + ';User Id=sa;Password=' + $sqlPw + ';Encrypt=False;TrustServerCertificate=True;'); if ('%CRONIQ_HOST_OTLP_ENDPOINT%') { $env:Croniq__Observability__OtlpEndpoint='%CRONIQ_HOST_OTLP_ENDPOINT%'; $env:Croniq__Observability__OtlpProtocol=$env:CRONIQ_OBS_OTLP_PROTOCOL }; $p = Start-Process -FilePath 'dotnet' -ArgumentList @('run','--project','samples\\Croniq.Sample.ApiHost\\Croniq.Sample.ApiHost.csproj') -WorkingDirectory (Resolve-Path '.') -RedirectStandardOutput '%OUT_LOG%' -RedirectStandardError '%ERR_LOG%' -PassThru; $p.Id | Out-File -FilePath '%PID_FILE%' -Encoding ascii" > "%START_LOG%" 2>&1
if errorlevel 1 (
    echo [devstack] Failed to start sample ApiHost.
    if exist "%START_LOG%" (
        echo [devstack] Last lines from %START_LOG%:
        powershell -NoProfile -Command "if (Test-Path '%START_LOG%') { Get-Content -Path '%START_LOG%' -Tail 60 }" 2>nul
    )
    endlocal & exit /b 1
)

echo [devstack] Sample ApiHost PID written to %PID_FILE%
echo [devstack] Waiting for host ApiHost at %HEALTH_URL% ...
call :wait_for_health "%HEALTH_URL%"
if errorlevel 1 (
    echo [devstack] Host ApiHost did not become healthy in time.
    if exist "%ERR_LOG%" (
        echo [devstack] Last lines from %ERR_LOG%:
        powershell -NoProfile -Command "if (Test-Path '%ERR_LOG%') { Get-Content -Path '%ERR_LOG%' -Tail 60 }" 2>nul
    )
    if exist "%OUT_LOG%" (
        echo [devstack] Last lines from %OUT_LOG%:
        powershell -NoProfile -Command "if (Test-Path '%OUT_LOG%') { Get-Content -Path '%OUT_LOG%' -Tail 60 }" 2>nul
    )
    endlocal & exit /b 1
)
endlocal & exit /b 0

:maybe_wait_for_api
setlocal
set PROFILE_ARGS=%~1
set HEALTH_URL=%~2
set RUN_SAMPLE=%~3
if not "%RUN_SAMPLE%"=="" (
    echo [devstack] Host ApiHost enabled. Skipping container API health probe.
    endlocal & exit /b 0
)
call :detect_profiles "%PROFILE_ARGS%"
if not "!HAS_API_PROFILE!"=="1" (
    echo [devstack] API profile disabled. Skipping health probe.
    endlocal & exit /b 0
)
echo [devstack] Waiting for API at %HEALTH_URL% ...
call :wait_for_health "%HEALTH_URL%"
if errorlevel 1 (
    echo [devstack] API did not become healthy in time.
    endlocal & exit /b 1
)
endlocal & exit /b 0

:detect_profiles
setlocal EnableDelayedExpansion
set "ARGS=%~1"
set HAS_API=0
set HAS_OBS=0
set PREV=

for %%T in (!ARGS!) do (
    if /I "!PREV!"=="--profile" (
        if /I "%%T"=="api" set HAS_API=1
        if /I "%%T"=="obs" set HAS_OBS=1
    )
    set PREV=%%T
)

endlocal & set "HAS_API_PROFILE=%HAS_API%" & set "HAS_OBS_PROFILE=%HAS_OBS%" & exit /b 0

:wait_for_health
setlocal
set URL=%~1
set /a ATTEMPTS=0
:poll
set /a ATTEMPTS+=1
if !ATTEMPTS! gtr 60 (endlocal & exit /b 1)
curl --silent --fail "%URL%" >nul 2>&1 && (endlocal & exit /b 0)
timeout /t 2 >nul
goto poll

:wait_for_migrator
setlocal enabledelayedexpansion
set COMPOSE_ARGS=%~1

echo [devstack] Waiting for DB migrator to complete...

REM Prefer `docker compose wait` if available (Compose v2.20+). If unsupported or failing,
REM fall back to polling.
docker compose %COMPOSE_ARGS% wait croniq-db-migrator >nul 2>&1
if not errorlevel 1 (endlocal & exit /b 0)

set /a ATTEMPTS=0
:migrator_poll
set /a ATTEMPTS+=1
if !ATTEMPTS! gtr 180 (
    echo [devstack] DB migrator did not complete in time.
    endlocal & exit /b 1
)

set MIGRATOR_CID=
for /f "usebackq delims=" %%C in (`docker compose %COMPOSE_ARGS% ps --all -q croniq-db-migrator 2^>nul`) do set MIGRATOR_CID=%%C

if not "!MIGRATOR_CID!"=="" (
    set MIGRATOR_STATUS=
    set MIGRATOR_EXIT=
    for /f "usebackq tokens=1,2 delims= " %%A in (`docker inspect -f "{{.State.Status}} {{.State.ExitCode}}" !MIGRATOR_CID! 2^>nul`) do (
        set MIGRATOR_STATUS=%%A
        set MIGRATOR_EXIT=%%B
    )

    if /I "!MIGRATOR_STATUS!"=="exited" (
        if "!MIGRATOR_EXIT!"=="0" (endlocal & exit /b 0)
        echo [devstack] DB migrator failed.
        endlocal & exit /b 1
    )
)

timeout /t 2 >nul
goto migrator_poll

:help
echo.
echo Croniq devstack up
echo.
echo Usage:
echo   scripts\devstack-up.cmd [--profile NAME ...] [--sample apihost]
echo.
echo Options:
echo   --profile NAME     Forwarded to docker compose. You can pass multiple.
echo                    Common profiles: api, worker, obs
echo   --sample apihost   Starts samples\Croniq.Sample.ApiHost on the host via dotnet run
echo                    after the DB migrator completed (and after API health check if api profile is enabled).
echo   --no-sample        Do not start the default host ApiHost.
echo   --help, -h, /?     Show this help.
echo.
echo Environment overrides:
echo   CRONIQ_DEVSTACK_PROFILES   If set, overrides all profile args passed to this script.
echo   CRONIQ_API_HTTP_PORT       Used for host ApiHost port and container API health probe (default: 5080).
echo   CRONIQ_API_BASEURL         Used for API health probe (default: http://localhost:%%CRONIQ_API_HTTP_PORT%%).
echo   CRONIQ_SAMPLE_APIHOST_HTTP_PORT  Informational only (default: 5090).
echo.
echo Examples:
echo   scripts\devstack-up.cmd
echo   scripts\devstack-up.cmd --profile api --profile obs
echo   scripts\devstack-up.cmd --profile worker --sample apihost
echo   scripts\devstack-up.cmd --profile worker --no-sample
echo.
exit /b 0
