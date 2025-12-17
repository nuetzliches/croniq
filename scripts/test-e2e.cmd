@echo off
setlocal enabledelayedexpansion

set COMPOSE_FILE=infra\docker\docker-compose.tests.yml
set TEST_PROJECT=tests\Croniq.Api.Smoke\Croniq.Api.Smoke.csproj

if "%CRONIQ_API_BASEURL%"=="" set CRONIQ_API_BASEURL=http://localhost:5080
if "%CRONIQ_API_KEY%"=="" set CRONIQ_API_KEY=smoke-key

set HEALTH_URL=%CRONIQ_API_BASEURL%
if not "%HEALTH_URL:~-1%"=="/" set HEALTH_URL=%HEALTH_URL%/
set HEALTH_URL=%HEALTH_URL%health

set EXITCODE=0

echo [1/5] Cleaning previous smoke stack...
docker compose -f "%COMPOSE_FILE%" down -v --remove-orphans >nul 2>&1

echo [2/5] Building Croniq smoke stack (%COMPOSE_FILE%)...
docker compose -f "%COMPOSE_FILE%" up --build -d
if errorlevel 1 (
	set EXITCODE=1
	goto fail_start
)

echo [3/5] Waiting for Croniq API at %HEALTH_URL% ...
call :wait_for_health "%HEALTH_URL%"
if errorlevel 1 goto fail_health

echo [4/5] Running Croniq.Api.Smoke tests...
dotnet test "%TEST_PROJECT%" --nologo
set EXITCODE=%ERRORLEVEL%

echo [5/5] Shutting down smoke stack...
docker compose -f "%COMPOSE_FILE%" down -v
exit /b %EXITCODE%

:fail_health
echo API did not become healthy in time.
set EXITCODE=1

:fail_start
docker compose -f "%COMPOSE_FILE%" down -v >nul 2>&1
exit /b %EXITCODE%

:wait_for_health
setlocal
set URL=%~1
set /a ATTEMPTS=0
:poll
set /a ATTEMPTS+=1
if !ATTEMPTS! gtr 60 (endlocal & exit /b 1)
curl --silent --fail "!URL!" >nul 2>&1 && (endlocal & exit /b 0)
timeout /t 2 >nul
goto poll
