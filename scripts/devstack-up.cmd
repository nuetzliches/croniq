@echo off
setlocal enabledelayedexpansion

set ROOT=%~dp0..
cd /d "%ROOT%" || exit /b 1

if not exist ".env" (
    echo [devstack] Missing .env in repo root. Copy .env.example to .env first.
    exit /b 1
)

set COMPOSE_ARGS=-f infra\docker\docker-compose.yml -f infra\docker\docker-compose.dev.yml -f infra\docker\docker-compose.observability.yml
set DEFAULT_PROFILES=--profile api --profile worker

if "%CRONIQ_API_HTTP_PORT%"=="" set CRONIQ_API_HTTP_PORT=5080
if "%CRONIQ_API_BASEURL%"=="" set CRONIQ_API_BASEURL=http://localhost:%CRONIQ_API_HTTP_PORT%
set HEALTH_URL=%CRONIQ_API_BASEURL%
if not "%HEALTH_URL:~-1%"=="/" set HEALTH_URL=%HEALTH_URL%/
set HEALTH_URL=%HEALTH_URL%health

set USER_PROFILES=%*

if not "%CRONIQ_DEVSTACK_PROFILES%"=="" (
    set PROFILE_ARGS=%CRONIQ_DEVSTACK_PROFILES%
) else (
    set PROFILE_ARGS=%DEFAULT_PROFILES% %USER_PROFILES%
)

echo [devstack] Starting Croniq services (%PROFILE_ARGS%)...
docker compose %COMPOSE_ARGS% %PROFILE_ARGS% up --build -d
if errorlevel 1 (
    echo [devstack] docker compose up failed.
    exit /b 1
)

call :maybe_wait_for_api "%PROFILE_ARGS%" "%HEALTH_URL%"
if errorlevel 1 exit /b 1

echo [devstack] Croniq dev stack is ready.
exit /b 0

:maybe_wait_for_api
setlocal
set PROFILE_ARGS=%~1
set HEALTH_URL=%~2
echo %PROFILE_ARGS% | findstr /I "--profile api" >nul
if errorlevel 1 (
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
