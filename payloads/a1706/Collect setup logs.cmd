@echo off
cd /d "%~dp0"
MacBookSetupLogger.exe --collect
echo.
echo Collection finished. Check the location shown above before closing this window.
pause
