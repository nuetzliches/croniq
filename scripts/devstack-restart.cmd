@echo off
setlocal enabledelayedexpansion

set ROOT=%~dp0..
cd /d "%ROOT%" || exit /b 1

REM Help / usage
if /I "%~1"=="--help" goto help
if /I "%~1"=="-h" goto help
if /I "%~1"=="/?" goto help

set DOWN_FORWARD_ARGS=
set UP_FORWARD_ARGS=

:parse
if "%~1"=="" goto parsed
if /I "%~1"=="--help" goto help
if /I "%~1"=="-h" goto help
if /I "%~1"=="/?" goto help
if /I "%~1"=="--sample" goto handle_sample
if /I "%~1"=="--no-sample" goto handle_no_sample
set DOWN_FORWARD_ARGS=!DOWN_FORWARD_ARGS! %~1
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! %~1
shift
goto parse

:handle_sample
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! %~1
shift
if "%~1"=="" goto parsed
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! %~1
shift
goto parse

:handle_no_sample
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! %~1
shift
goto parse

:parsed

echo [devstack] Restarting Croniq services (!UP_FORWARD_ARGS!)...
REM Always drop volumes on restart to avoid stale SQL schemas between migrations.
call scripts\devstack-down.cmd !DOWN_FORWARD_ARGS! --remove-orphans -v
if errorlevel 1 (
    echo [devstack] docker compose down failed.
    exit /b 1
)

call scripts\devstack-up.cmd !UP_FORWARD_ARGS!
exit /b %ERRORLEVEL%

:help
echo.
echo Croniq devstack restart
echo.
echo Usage:
echo   scripts\devstack-restart.cmd [--profile NAME ...] [--sample apihost] [--no-sample]
echo.
echo Notes:
echo   - --profile is forwarded to docker compose (via devstack-down/up).
echo   - --sample is forwarded only to devstack-up.cmd (host-run samples).
echo   - --no-sample is forwarded only to devstack-up.cmd.
echo   - Restart always drops volumes (-v) to avoid stale SQL schemas.
echo.
echo Examples:
echo   scripts\devstack-restart.cmd
echo   scripts\devstack-restart.cmd --profile api --profile obs
echo   scripts\devstack-restart.cmd --profile worker --sample apihost
echo   scripts\devstack-restart.cmd --profile worker --no-sample
echo.
exit /b 0
