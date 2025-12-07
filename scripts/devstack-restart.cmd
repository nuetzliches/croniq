@echo off
setlocal

set ROOT=%~dp0..
cd /d "%ROOT%" || exit /b 1

set USER_PROFILES=%*

echo [devstack] Restarting Croniq services (%USER_PROFILES%)...
call scripts\devstack-down.cmd %USER_PROFILES% --remove-orphans
if errorlevel 1 (
    echo [devstack] docker compose down failed.
    exit /b 1
)

call scripts\devstack-up.cmd %USER_PROFILES%
exit /b %ERRORLEVEL%
