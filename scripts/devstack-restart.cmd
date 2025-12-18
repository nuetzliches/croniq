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
set CLEAN=
set BUILD_MODE=

:parse
if "%~1"=="" goto parsed
if /I "%~1"=="--help" goto help
if /I "%~1"=="-h" goto help
if /I "%~1"=="/?" goto help
if /I "%~1"=="--clean" goto handle_clean
if /I "%~1"=="--build" goto handle_build
if /I "%~1"=="--no-build" goto handle_no_build
if /I "%~1"=="--sample" goto handle_sample
if /I "%~1"=="--no-sample" goto handle_no_sample
if /I "%~1"=="--follow" goto handle_follow
if /I "%~1"=="--no-follow" goto handle_no_follow
if /I "%~1"=="--no-window" goto handle_no_window
if /I "%~1"=="--window" goto handle_window
set DOWN_FORWARD_ARGS=!DOWN_FORWARD_ARGS! %~1
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! %~1
shift
goto parse

:handle_clean
set CLEAN=1
shift
goto parse

:handle_build
set BUILD_MODE=--build
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! --build
shift
goto parse

:handle_no_build
set BUILD_MODE=--no-build
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! --no-build
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

:handle_follow
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! %~1
shift
goto parse

:handle_no_follow
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! %~1
shift
goto parse

:handle_no_window
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! %~1
shift
goto parse

:handle_window
set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! %~1
shift
goto parse

:parsed

REM Default to fast restart: no rebuild unless requested.
if "%BUILD_MODE%"=="" (
    set UP_FORWARD_ARGS=!UP_FORWARD_ARGS! --no-build
)

echo [devstack] Restarting Croniq services (!UP_FORWARD_ARGS!)...
REM Default: keep volumes for faster restarts. Use --clean to drop volumes.
if "%CLEAN%"=="1" (
    call scripts\devstack-down.cmd !DOWN_FORWARD_ARGS! --remove-orphans -v
) else (
    call scripts\devstack-down.cmd !DOWN_FORWARD_ARGS! --remove-orphans
)
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
echo   - --follow/--no-follow are forwarded only to devstack-up.cmd.
echo   - --no-window/--window are forwarded only to devstack-up.cmd.
echo   - Restart keeps volumes by default for speed; use --clean to drop volumes (-v).
echo   - Restart skips docker builds by default; use --build to rebuild images.
echo.
echo Examples:
echo   scripts\devstack-restart.cmd
echo   scripts\devstack-restart.cmd --no-build
echo   scripts\devstack-restart.cmd --build
echo   scripts\devstack-restart.cmd --clean --build
echo   scripts\devstack-restart.cmd --profile api --profile obs
echo   scripts\devstack-restart.cmd --profile worker --sample apihost
echo   scripts\devstack-restart.cmd --profile worker --no-sample
echo.
exit /b 0
