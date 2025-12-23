@echo off
setlocal enabledelayedexpansion

set SCRIPT_DIR=%~dp0
set ROOT=%~dp0..
cd /d "%ROOT%" || exit /b 1

if not exist ".env" (
    echo [croniq-trigger] Missing .env in repo root. Copy .env.example to .env first.
    exit /b 1
)

set API_PORT=%CRONIQ_API_HTTP_PORT%
if "%API_PORT%"=="" set API_PORT=5080

set API_BASE=%CRONIQ_API_BASEURL%
if "%API_BASE%"=="" set API_BASE=http://localhost:%API_PORT%
if not "%API_BASE:~-1%"=="/" set API_BASE=%API_BASE%/

set API_KEY=%CRONIQ_SMOKE_API_KEY%
if "%API_KEY%"=="" set API_KEY=smoke-key

set TENANT_ID=%CRONIQ_CORE_TENANT_REFERENCE%
if "%TENANT_ID%"=="" set TENANT_ID=1

set ENVIRONMENT_TAG=%CRONIQ_CORE_ENVIRONMENT%
if "%ENVIRONMENT_TAG%"=="" set ENVIRONMENT_TAG=dev

set JOB_KEY=%~1
if "%JOB_KEY%"=="" set JOB_KEY=%TENANT_ID%:%ENVIRONMENT_TAG%:samples:smoke

set METADATA_TAG=%~2
if "%METADATA_TAG%"=="" set METADATA_TAG=devstack-script

set ENDPOINT=%API_BASE%jobs/trigger

echo [croniq-trigger] POST %ENDPOINT%
echo [croniq-trigger] JobKey=%JOB_KEY%

powershell -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%devstack-trigger-job.ps1" ^
    -JobKey "%JOB_KEY%" ^
    -ApiKey "%API_KEY%" ^
    -Endpoint "%ENDPOINT%" ^
    -Initiator "%METADATA_TAG%"
if errorlevel 1 (
    echo.
    echo [croniq-trigger] Job trigger failed.
    exit /b 1
)

echo.
echo [croniq-trigger] Job trigger accepted.
exit /b 0
