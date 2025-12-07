@echo off
setlocal enabledelayedexpansion

set ROOT=%~dp0..
cd /d "%ROOT%" || exit /b 1

set COMPOSE_ARGS=-f infra\docker\docker-compose.yml -f infra\docker\docker-compose.dev.yml -f infra\docker\docker-compose.observability.yml
set DEFAULT_PROFILES=--profile api --profile worker
set PROFILE_ARGS=%DEFAULT_PROFILES%
set DOWN_ARGS=

:parse
if "%~1"=="" goto run
if /I "%~1"=="--profile" goto handle_profile
set DOWN_ARGS=!DOWN_ARGS! %~1
shift
goto parse

:handle_profile
shift
if "%~1"=="" goto run
set PROFILE_ARGS=!PROFILE_ARGS! --profile %~1
shift
goto parse

:run
echo [devstack] Stopping Croniq services...
docker compose %COMPOSE_ARGS% !PROFILE_ARGS! down !DOWN_ARGS!
exit /b %ERRORLEVEL%
