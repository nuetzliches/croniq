@echo off
setlocal EnableExtensions DisableDelayedExpansion

set ROOT=%~dp0..
cd /d "%ROOT%" || exit /b 1

set "COMPOSE_ARGS=-f infra\docker\docker-compose.yml -f infra\docker\docker-compose.dev.yml -f infra\docker\docker-compose.observability.yml"
set "DEFAULT_PROFILES=--profile api --profile worker"
set "PROFILE_ARGS=%DEFAULT_PROFILES%"
set "DOWN_ARGS="

REM Help / usage
if /I "%~1"=="--help" goto help
if /I "%~1"=="-h" goto help
if /I "%~1"=="/?" goto help

:parse
if "%~1"=="" goto run
if /I "%~1"=="--help" goto help
if /I "%~1"=="-h" goto help
if /I "%~1"=="/?" goto help
if /I "%~1"=="--ui" goto ignore_ui
if /I "%~1"=="--no-ui" goto ignore_ui
if /I "%~1"=="--sample" goto ignore_sample
if /I "%~1"=="--profile" goto handle_profile
set "DOWN_ARGS=%DOWN_ARGS% %~1"
shift
goto parse

:ignore_ui
shift
goto parse

:ignore_sample
shift
if not "%~1"=="" shift
goto parse

:handle_profile
shift
if "%~1"=="" goto run
set "PROFILE_ARGS=%PROFILE_ARGS% --profile %~1"
shift
goto parse

:run
echo [devstack] Stopping Croniq services...
call :ensure_docker_engine
if errorlevel 1 (
  set DOCKER_EXIT=1
  goto after_compose
)

docker.exe compose %COMPOSE_ARGS% %PROFILE_ARGS% down %DOWN_ARGS%
set DOCKER_EXIT=%ERRORLEVEL%

REM Stop optional host-run samples (best-effort).
:after_compose
call :stop_sample_apihost
call :stop_ui

exit /b %DOCKER_EXIT%

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

:stop_sample_apihost
setlocal
set "PID_FILE=artifacts\devstack\sample-apihost.pid"
if not exist "%PID_FILE%" goto stop_sample_apihost_done

set "SAMPLE_PID="
set /p SAMPLE_PID=<"%PID_FILE%"

if "%SAMPLE_PID%"=="" goto stop_sample_apihost_delete

set "INVALID_PID="
for /f "delims=0123456789" %%A in ("%SAMPLE_PID%") do set "INVALID_PID=1"

if defined INVALID_PID (
  echo [devstack] Warning: PID file contained an unexpected value. Skipping process termination.
  goto stop_sample_apihost_delete
)

echo [devstack] Stopping sample ApiHost (PID %SAMPLE_PID%)...
taskkill /PID %SAMPLE_PID% /F >nul 2>&1

:stop_sample_apihost_delete

del /q "%PID_FILE%" >nul 2>&1
:stop_sample_apihost_done
endlocal & exit /b 0

:stop_ui
setlocal
set "PID_FILE=artifacts\devstack\ui.pid"
if not exist "%PID_FILE%" goto stop_ui_done

set "UI_PID="
set /p UI_PID=<"%PID_FILE%"

if "%UI_PID%"=="" goto stop_ui_delete

set "INVALID_PID="
for /f "delims=0123456789" %%A in ("%UI_PID%") do set "INVALID_PID=1"

if defined INVALID_PID (
  echo [devstack] Warning: UI PID file contained an unexpected value. Skipping process termination.
  goto stop_ui_delete
)

echo [devstack] Stopping UI terminal (PID %UI_PID%)...
taskkill /PID %UI_PID% /F >nul 2>&1

:stop_ui_delete
del /q "%PID_FILE%" >nul 2>&1
:stop_ui_done
endlocal & exit /b 0

:help
echo.
echo Croniq devstack down
echo.
echo Usage:
echo   scripts\devstack-down.cmd [--profile NAME ...] [docker compose down args]
echo.
echo Notes:
echo   - Profiles are forwarded to docker compose.
echo   - This script also stops host-run processes started via devstack-up.cmd (best-effort).
echo.
echo Examples:
echo   scripts\devstack-down.cmd
echo   scripts\devstack-down.cmd --profile api --remove-orphans -v
echo.
exit /b 0
