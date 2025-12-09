@echo off
setlocal

set ROOT=%~dp0..
cd /d "%ROOT%" || exit /b 1

set USER_PROFILES=%*

echo [devstack] Restarting Croniq services (%USER_PROFILES%)...
REM Always drop volumes on restart to avoid stale SQL schemas between migrations.
call scripts\devstack-down.cmd %USER_PROFILES% --remove-orphans -v
if errorlevel 1 (
    echo [devstack] docker compose down failed.
    exit /b 1
)

call scripts\devstack-up.cmd %USER_PROFILES%
exit /b %ERRORLEVEL%
