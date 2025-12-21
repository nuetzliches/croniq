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
set DO_BUILD=1
set FOLLOW_SAMPLE=
set NO_SAMPLE_WINDOW=

set START_UI=
set NO_UI=

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
if /I "%~1"=="--ui" goto handle_ui
if /I "%~1"=="--no-ui" goto handle_no_ui
if /I "%~1"=="--build" goto handle_build
if /I "%~1"=="--no-build" goto handle_no_build
if /I "%~1"=="--follow" goto handle_follow
if /I "%~1"=="--no-follow" goto handle_no_follow
if /I "%~1"=="--no-window" goto handle_no_window
if /I "%~1"=="--window" goto handle_window
if /I "%~1"=="--no-sample" goto handle_no_sample
if /I "%~1"=="--sample" goto handle_sample
set USER_PROFILES=%USER_PROFILES% %~1
shift
goto parse

:handle_ui
set START_UI=1
set NO_UI=
shift
goto parse

:handle_no_ui
set NO_UI=1
set START_UI=
shift
goto parse

:handle_build
set DO_BUILD=1
shift
goto parse

:handle_no_build
set DO_BUILD=0
shift
goto parse

:handle_follow
set FOLLOW_SAMPLE=1
shift
goto parse

:handle_no_follow
set FOLLOW_SAMPLE=0
shift
goto parse

:handle_no_window
set NO_SAMPLE_WINDOW=1
shift
goto parse

:handle_window
set NO_SAMPLE_WINDOW=0
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

REM Default behavior: if we start a host sample, follow its log so the console stays attached.
if not "%RUN_SAMPLE%"=="" (
    if "%FOLLOW_SAMPLE%"=="" set FOLLOW_SAMPLE=1
    if "%NO_SAMPLE_WINDOW%"=="" set NO_SAMPLE_WINDOW=1
) else (
    if "%FOLLOW_SAMPLE%"=="" set FOLLOW_SAMPLE=0
    if "%NO_SAMPLE_WINDOW%"=="" set NO_SAMPLE_WINDOW=0
)

REM Guard: do not allow container api and host api simultaneously.
if /I "%RUN_SAMPLE%"=="apihost" (
    if "!HAS_API_PROFILE!"=="1" (
        echo [devstack] Invalid configuration: Host ApiHost --sample apihost cannot be combined with container profile 'api'.
        echo [devstack] Remove '--profile api' or set --no-sample to run container api instead.
        exit /b 1
    )
)

REM Decide whether to start the UI (dev-only) in a separate terminal.
set UI_REASON=
set AUTO_UI=0
set UI_ENABLED=

REM Hard-disable in CI / GitHub Actions (even if --ui is passed).
if not "%GITHUB_ACTIONS%"=="" (
    set UI_ENABLED=0
    set UI_REASON=disabled because running in GitHub Actions
) else if not "%CI%"=="" (
    set UI_ENABLED=0
    set UI_REASON=disabled because running in CI
)

REM Allow env override for local dev.
if "%UI_ENABLED%"=="" (
    if /I "%CRONIQ_DEVSTACK_UI%"=="0" set UI_ENABLED=0
    if /I "%CRONIQ_DEVSTACK_UI%"=="false" set UI_ENABLED=0
    if /I "%CRONIQ_DEVSTACK_UI%"=="1" set UI_ENABLED=1
    if /I "%CRONIQ_DEVSTACK_UI%"=="true" set UI_ENABLED=1
    if not "%UI_ENABLED%"=="" set UI_REASON=override via CRONIQ_DEVSTACK_UI=%CRONIQ_DEVSTACK_UI%
)

REM Command line opts win over env (except CI hard-disable above).
if "%UI_ENABLED%"=="" (
    if not "%START_UI%"=="" (
        set UI_ENABLED=1
        set UI_REASON=explicit --ui
    ) else if not "%NO_UI%"=="" (
        set UI_ENABLED=0
        set UI_REASON=disabled via --no-ui
    ) else (
        set UI_ENABLED=1
        set AUTO_UI=1
        set UI_REASON=enabled by default
    )
)

REM Guard: UI needs an API (either host sample or container profile).
if "%UI_ENABLED%"=="1" (
    if not "%RUN_SAMPLE%"=="" (
        REM Host ApiHost will be started; OK.
    ) else (
        if not "!HAS_API_PROFILE!"=="1" (
            set UI_ENABLED=0
            set UI_REASON=skipped because no API is started (no host sample, no api profile)
        )
    )
)

if "%UI_ENABLED%"=="1" (
    echo [devstack] UI will start: %UI_REASON%
) else (
    echo [devstack] UI not started: %UI_REASON%
)

set BUILD_ARG=--build
if "%DO_BUILD%"=="0" set BUILD_ARG=

echo [devstack] Starting Croniq services (%PROFILE_ARGS%)...
call :ensure_docker_engine
if errorlevel 1 exit /b 1
docker compose %COMPOSE_ARGS% %PROFILE_ARGS% up %BUILD_ARG% -d
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

call :maybe_start_ui "%UI_ENABLED%"
if errorlevel 1 exit /b 1

echo [devstack] Croniq dev stack is ready.

call :maybe_follow_sample "%RUN_SAMPLE%" "%FOLLOW_SAMPLE%"
if errorlevel 1 exit /b 1

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
del /q "%START_LOG%" >nul 2>&1
del /q "%OUT_LOG%" >nul 2>&1
del /q "%ERR_LOG%" >nul 2>&1

set "PS_WINDOW_ARGS="
if "%NO_SAMPLE_WINDOW%"=="1" set "PS_WINDOW_ARGS=-WindowStyle Hidden"

set PS_EXIT=0
setlocal DisableDelayedExpansion
powershell -NoProfile -Command "$env:ASPNETCORE_URLS='http://0.0.0.0:%CRONIQ_SAMPLE_APIHOST_HTTP_PORT%'; if ($env:CRONIQ_DOTNET_ENVIRONMENT) { $env:DOTNET_ENVIRONMENT=$env:CRONIQ_DOTNET_ENVIRONMENT }; $env:Croniq__Auth__Mode=$env:CRONIQ_AUTH_MODE; $env:Croniq__Auth__InMemory__ApiKey=$env:CRONIQ_SMOKE_API_KEY; $env:Croniq__Auth__InMemory__TenantId=$env:CRONIQ_CORE_TENANT_ID; $env:Croniq__Auth__InMemory__EnvironmentTag=$env:CRONIQ_CORE_ENVIRONMENT; $env:Croniq__Persistence__Mode='SqlServer'; $env:Croniq__Api__RequestsPerMinute=$env:CRONIQ_API_REQUESTS_PER_MINUTE; $env:Croniq__Core__TenantId=$env:CRONIQ_CORE_TENANT_ID; $env:Croniq__Core__EnvironmentTag=$env:CRONIQ_CORE_ENVIRONMENT; $env:Croniq__Core__InstanceId=$env:CRONIQ_API_INSTANCE_ID; $dotenv=@{}; if (Test-Path '.env') { Get-Content '.env' | ForEach-Object { $l=$_.Trim(); if (!$l -or $l.StartsWith('#')) { return }; $parts=$l -split '=',2; if ($parts.Count -eq 2) { $dotenv[$parts[0].Trim()]=$parts[1].Trim() } } }; function Get-EnvOrDotenv([string]$k,[string]$fallback) { $v=[Environment]::GetEnvironmentVariable($k); if ($v) { return $v }; if ($dotenv.ContainsKey($k) -and $dotenv[$k]) { return $dotenv[$k] }; return $fallback }; $sqlPort = Get-EnvOrDotenv 'CRONIQ_SQL_HOST_PORT' '11433'; $sqlDb = Get-EnvOrDotenv 'CRONIQ_SQL_DATABASE' 'CroniqDev'; $sqlPw = Get-EnvOrDotenv 'CRONIQ_SQL_PASSWORD' 'CroniqSqlP@ssw0rd!'; $env:Croniq__SqlServer__ConnectionString = ('Server=localhost,' + $sqlPort + ';Database=' + $sqlDb + ';User Id=sa;Password=' + $sqlPw + ';Encrypt=False;TrustServerCertificate=True;'); if ('%CRONIQ_HOST_OTLP_ENDPOINT%') { $env:Croniq__Observability__OtlpEndpoint='%CRONIQ_HOST_OTLP_ENDPOINT%'; $env:Croniq__Observability__OtlpProtocol=$env:CRONIQ_OBS_OTLP_PROTOCOL }; $p = Start-Process -FilePath 'dotnet' -ArgumentList @('run','--project','samples\Croniq.Sample.ApiHost\Croniq.Sample.ApiHost.csproj') -WorkingDirectory (Resolve-Path '.') %PS_WINDOW_ARGS% -RedirectStandardOutput '%OUT_LOG%' -RedirectStandardError '%ERR_LOG%' -PassThru; $p.Id | Out-File -FilePath '%PID_FILE%' -Encoding ascii" > "%START_LOG%" 2>&1
set PS_EXIT=%ERRORLEVEL%
endlocal & set PS_EXIT=%PS_EXIT%

if not "%PS_EXIT%"=="0" (
    echo [devstack] Failed to start sample ApiHost.
    if exist "%START_LOG%" (
        echo [devstack] Last lines from %START_LOG%:
        powershell -NoProfile -Command "if (Test-Path '%START_LOG%') { Get-Content -Encoding UTF8 -Path '%START_LOG%' -Tail 60 }" 2>nul
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
        powershell -NoProfile -Command "if (Test-Path '%ERR_LOG%') { Get-Content -Encoding UTF8 -Path '%ERR_LOG%' -Tail 80 }" 2>nul
    )
    if exist "%OUT_LOG%" (
        echo [devstack] Last lines from %OUT_LOG%:
        powershell -NoProfile -Command "if (Test-Path '%OUT_LOG%') { Get-Content -Encoding UTF8 -Path '%OUT_LOG%' -Tail 80 }" 2>nul
    )
    endlocal & exit /b 1
)
endlocal & exit /b 0

:maybe_follow_sample
setlocal EnableExtensions DisableDelayedExpansion
set "SAMPLE=%~1"
set "FOLLOW=%~2"
set "SAMPLE=%SAMPLE:"=%"

if "%SAMPLE%"=="" (endlocal & exit /b 0)
if /I not "%SAMPLE%"=="apihost" (endlocal & exit /b 0)
if not "%FOLLOW%"=="1" (endlocal & exit /b 0)

set "OUT_LOG=artifacts\devstack\sample-apihost.out.log"
set "ERR_LOG=artifacts\devstack\sample-apihost.err.log"

if not exist "%OUT_LOG%" if not exist "%ERR_LOG%" (
    echo [devstack] ApiHost log files not found.
    endlocal & exit /b 0
)

echo [devstack] Following host ApiHost logs (Ctrl+C to stop following; ApiHost keeps running)...
powershell -NoProfile -Command "Get-Content -Encoding UTF8 -Path @('%OUT_LOG%','%ERR_LOG%') -Tail 50 -Wait"
endlocal & exit /b 0

:maybe_start_ui
setlocal EnableExtensions DisableDelayedExpansion
set "ENABLE=%~1"

if not "%ENABLE%"=="1" (endlocal & exit /b 0)

REM We start the UI in a separate PowerShell window (dev-only).
set "UI_DIR=src\Croniq.Ui"
if not exist "%UI_DIR%\package.json" (
    echo [devstack] UI not started: %UI_DIR%\package.json not found.
    endlocal & exit /b 0
)

REM Best-effort check for npm.
where npm >nul 2>&1
if errorlevel 1 (
    echo [devstack] UI not started: npm not found on PATH.
    echo [devstack] Install Node.js + npm, then rerun devstack-up.
    endlocal & exit /b 0
)

set "PID_DIR=artifacts\devstack"
if not exist "%PID_DIR%" mkdir "%PID_DIR%" >nul 2>&1
set "PID_FILE=%PID_DIR%\ui.pid"

REM Stop previous UI instance if pid file exists.
if exist "%PID_FILE%" (
    for /f "usebackq delims=" %%P in ("%PID_FILE%") do set OLD_PID=%%P
    if not "%OLD_PID%"=="" (
        taskkill /PID %OLD_PID% /T /F >nul 2>&1
    )
    del /q "%PID_FILE%" >nul 2>&1
)

REM UI endpoint (Angular default).
if "%CRONIQ_UI_HTTP_PORT%"=="" set CRONIQ_UI_HTTP_PORT=5081
echo [devstack] Starting UI (Angular) in a separate terminal/window: http://localhost:%CRONIQ_UI_HTTP_PORT% ...

set "UI_SCRIPT=scripts\devstack-ui.ps1"
if not exist "%UI_SCRIPT%" (
    echo [devstack] UI not started: %UI_SCRIPT% not found.
    endlocal & exit /b 0
)

REM Prefer Windows Terminal new-tab when running inside Windows Terminal.
where wt >nul 2>&1
if not errorlevel 1 (
    if not "%WT_SESSION%"=="" (
        wt -w 0 new-tab --title "Croniq UI" -d "%CD%" powershell -NoProfile -NoExit -ExecutionPolicy Bypass -File "%CD%\%UI_SCRIPT%" -UiPort %CRONIQ_UI_HTTP_PORT% >nul 2>&1
        goto wait_for_ui_pid
    )
)

REM Fallback: start a separate PowerShell window.
powershell -NoProfile -Command "$root=(Resolve-Path '.'); $script=Join-Path $root '%UI_SCRIPT%'; $args=@('-NoProfile','-NoExit','-ExecutionPolicy','Bypass','-File',$script,'-UiPort','%CRONIQ_UI_HTTP_PORT%'); Start-Process -FilePath 'powershell' -ArgumentList $args -WorkingDirectory $root | Out-Null" >nul 2>&1

:wait_for_ui_pid
REM Wait briefly for PID file to be created by scripts/devstack-ui.ps1.
for /L %%I in (1,1,20) do (
    if exist "%PID_FILE%" goto ui_pid_ok
    timeout /t 1 >nul
)

echo [devstack] Warning: UI PID file was not created. UI may still be running.
endlocal & exit /b 0

:ui_pid_ok
echo [devstack] UI PID written to %PID_FILE%
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

:ensure_docker_engine
setlocal
docker.exe info >nul 2>&1
if errorlevel 1 (
    echo [devstack] Docker engine not reachable.
    echo [devstack] Start Docker Desktop and try again.
    echo [devstack] If this persists, verify the Docker context with: docker context ls
    endlocal & exit /b 1
)
endlocal & exit /b 0

:help
echo.
echo Croniq devstack up
echo.
echo Usage:
echo   scripts\devstack-up.cmd [--profile NAME ...] [--sample apihost] [--ui ^| --no-ui]
echo.
echo Options:
echo   --profile NAME     Forwarded to docker compose. You can pass multiple.
echo                    Common profiles: api, worker, obs
echo   --ui               Start Croniq.Ui (Angular) in a separate terminal (dev-only).
echo   --no-ui            Do not start Croniq.Ui.
echo   --build            Build docker images before starting containers (default).
echo   --no-build         Skip docker image build and just start existing images.
echo   --follow           If a host sample is started, follow its log output (default).
echo   --no-follow        Do not follow host sample logs; exit after printing 'ready'.
echo   --no-window        Start host sample without a visible window (default).
echo   --window           Start host sample with a normal window (debug).
echo   --sample apihost   Starts samples\Croniq.Sample.ApiHost on the host via dotnet run
echo                    after the DB migrator completed (and after API health check if api profile is enabled).
echo   --no-sample        Do not start the default host ApiHost.
echo   --help, -h, /?     Show this help.
echo.
echo Environment overrides:
echo   CRONIQ_DEVSTACK_PROFILES   If set, overrides all profile args passed to this script.
echo   CRONIQ_DEVSTACK_UI         If set (0/1, false/true), overrides starting the UI (local dev only; CI is always disabled).
echo   CRONIQ_API_HTTP_PORT       Used for host ApiHost port and container API health probe (default: 5080).
echo   CRONIQ_API_BASEURL         Used for API health probe (default: http://localhost:%%CRONIQ_API_HTTP_PORT%%).
echo   CRONIQ_UI_HTTP_PORT        UI port for ng serve (default: 5081).
echo   CRONIQ_SAMPLE_APIHOST_HTTP_PORT  Informational only (default: 5090).
echo.
echo Examples:
echo   scripts\devstack-up.cmd
echo   scripts\devstack-up.cmd --no-ui
echo   scripts\devstack-up.cmd --ui
echo   scripts\devstack-up.cmd --profile api --profile obs
echo   scripts\devstack-up.cmd --profile worker --sample apihost
echo   scripts\devstack-up.cmd --profile worker --no-sample
echo.
exit /b 0
