@echo off
setlocal EnableDelayedExpansion
if exist X:\a1706.ran goto :eof
echo ran>X:\a1706.ran
set "LOGGER=%SystemRoot%\System32\MacBookSetupLogger.exe"
"%LOGGER%" --start
set "PHASE=initialization"
set "MSG=A1706 setup log v17 %DATE% %TIME%"
call :note
set "MSG=USB logs: MacBookPatcher-Logs on explicitly marked USB storage; RAM-only logs are lost at reboot"
call :note
set "PHASE=wpeinit"
"%LOGGER%" --event wpeinit-start
wpeinit
set RC=!ERRORLEVEL!
set "MSG=wpeinit finished errorlevel=!RC!"
call :note
call :copysapi
set "MSG=step after-wpeinit"
call :note
set "MSG=v17 restart iris only"
call :note
call :dump pnputil /enum-devices /class Display
call :dump pnputil /enum-devices /problem
call :copysapi
set "PHASE=gpu-discovery"
set "GPU="
"%LOGGER%" --event gpu-discovery-start
%SystemRoot%\System32\a1706gpu.exe >X:\a1706-gpu-id.txt 2>X:\a1706-gpu-error.txt
set "GPU_RC=!ERRORLEVEL!"
call :addfile X:\a1706-gpu-error.txt
if not "!GPU_RC!"=="0" goto :gpufailed
set /p GPU=<X:\a1706-gpu-id.txt
if not defined GPU goto :gpufailed
set "MSG=GPU detected !GPU!"
call :note
call :gpurestart
set DRV=
for %%L in (C D E F G H I J K L M N O P Q R S T U V W Y Z) do if exist %%L:\ call :finddrv %%L
set "MSG=disc=!DRV!"
call :note
if not defined DRV (
  set "MSG=ERROR missing WinPEDriver"
  call :note
  goto :afterdrv
)
set "TAG=cirrus4208"
set "ONE=!DRV!\CirrusAudioCS4208x64\cs4208_36.inf"
call :loadone
call :cue speakers
set "TAG=keyboard"
set "ONE=!DRV!\AppleSPIKeyboard\AppleSPIKeyboard.inf"
call :loadone
call :cue keyboard
set "TAG=touchbar"
set "ONE=!DRV!\AppleDFR64\AppleDFR.inf"
call :loadone
:afterdrv
rem Limit device diagnostics to display, monitor and problem devices.
call :dump pnputil /enum-devices /problem
call :dump pnputil /enum-devices /class Display
call :dump pnputil /enum-devices /class Monitor
call :dump pnputil /enum-drivers
call :dump reg query HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY /s
call :dump reg query HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers /s
dir %SystemRoot%\System32\DisplaySwitch.exe >X:\a1706-cmd-out.txt 2>&1
call :addfile X:\a1706-cmd-out.txt
if exist %SystemRoot%\System32\DisplaySwitch.exe (
  set "MSG=DisplaySwitch.exe present"
  call :note
) else (
  set "MSG=ERROR missing DisplaySwitch.exe"
  call :note
)
call :copysapi
if exist %SystemRoot%\System32\Narrator.exe start "" %SystemRoot%\System32\Narrator.exe
set "PHASE=iris-readiness"
set "N=0"
set "IRISOK=0"
:waitiris
set /a N+=1
pnputil /enum-devices /class Display >X:\a1706-cmd-out.txt 2>&1
call :addfile X:\a1706-cmd-out.txt
set "MSG=iris wait !N! %TIME%"
call :note
if not exist %SystemRoot%\System32\a1706iris.exe (
  set "MSG=ERROR missing a1706iris.exe"
  call :note
  goto :irisdone
)
%SystemRoot%\System32\a1706iris.exe
if !ERRORLEVEL! equ 0 (
  set "MSG=iris started %TIME%"
  call :note
  set "IRISOK=1"
  goto :irisdone
)
if !N! geq 30 goto :irisdone
ping -n 3 127.0.0.1 >nul
goto :waitiris
:irisdone
if not "!IRISOK!"=="1" (
  "%LOGGER%" --event iris-readiness-failed "Iris did not reach Started"
  set "MSG=ERROR iris not started"
  call :note
  echo Iris did not reach Started. Setup has not been launched.
  echo Keep the marked logging USB connected and save the support folder.
  pause
  goto :eof
)
if exist %SystemRoot%\System32\DisplaySwitch.exe (
  "%LOGGER%" --event display-switch-start
  %SystemRoot%\System32\DisplaySwitch.exe /external
  "%LOGGER%" --event display-switch-return "!ERRORLEVEL!"
)
set "SRC="
for %%L in (C D E F G H I J K L M N O P Q R S T U V W Y Z) do (
  if exist "%%L:\sources\install.wim" (
    if not defined SRC set "SRC=%%L:"
  )
)
if not defined SRC (
  set "MSG=ERROR missing install.wim"
  call :note
) else (
  set "MSG=installfrom !SRC!\sources\install.wim"
  call :note
)
set "MSG=Setup dialog and Panther collection running"
call :note
set "PHASE=windows-setup"
set "MSG=launching setup %TIME%"
call :note
if exist X:\sources\setup.exe (
  if defined SRC (
    X:\sources\setup.exe /InstallFrom:!SRC!\sources\install.wim
  ) else (
    X:\sources\setup.exe
  )
) else (
  if defined SRC (
    %SYSTEMDRIVE%\sources\setup.exe /InstallFrom:!SRC!\sources\install.wim
  ) else (
    %SYSTEMDRIVE%\sources\setup.exe
  )
)
set "SETUP_RC=!ERRORLEVEL!"
"%LOGGER%" --event setup-exit "!SETUP_RC!"
"%LOGGER%" --stop
goto :eof
:gpufailed
"%LOGGER%" --event gpu-discovery-failed "!GPU_RC!"
echo No unique A1706 Iris 550 device was detected. Setup has not been launched.
echo Keep the marked logging USB connected and save MacBookPatcher-Logs.
pause
goto :eof
:finddrv
set "MSG=check %~1"
call :note
if exist "%~1:\$WinPEDriver$\AppleSPIKeyboard\AppleSPIKeyboard.inf" (
  set "DRV=%~1:\$WinPEDriver$"
  set "MSG=winpedriver-on %~1"
  call :note
)
goto :eof
:loadone
set "PHASE=driver-load-!TAG!"
set "MSG=step load !TAG!"
call :note
if not exist "!ONE!" (
  set "MSG=ERROR missing !ONE!"
  call :note
  goto :eof
)
set "MSG=LOAD !ONE!"
call :note
drvload "!ONE!" >X:\a1706-cmd-out.txt 2>&1
set RC=!ERRORLEVEL!
call :addfile X:\a1706-cmd-out.txt
if !RC! equ 0 (
  set "MSG=errorlevel=!RC! loaded !TAG!"
) else (
  set "MSG=ERROR errorlevel=!RC! !TAG!"
)
call :note
call :copysapi
goto :eof
:gpurestart
set "PHASE=gpu-restart"
set "MSG=pnputil restart-device"
call :note
pnputil /restart-device "!GPU!" >X:\a1706-cmd-out.txt 2>&1
set RC=!ERRORLEVEL!
call :addfile X:\a1706-cmd-out.txt
set "MSG=restart errorlevel=!RC!"
call :note
call :dump pnputil /enum-devices /class Display
call :dump pnputil /enum-devices /problem
call :copysapi
goto :eof
:dump
set "MSG=----- %* -----"
call :note
%* >X:\a1706-cmd-out.txt 2>&1
set RC=!ERRORLEVEL!
call :addfile X:\a1706-cmd-out.txt
if !RC! equ 0 (
  set "MSG=dump errorlevel=!RC!"
) else (
  set "MSG=ERROR dump errorlevel=!RC!"
)
call :note
goto :eof
:copysapi
if not exist "%SystemRoot%\inf\setupapi.dev.log" (
  set "MSG=ERROR missing setupapi.dev.log"
  call :note
  goto :eof
)
rem The background collector snapshots SetupAPI and Panther independently.
set "MSG=copied setupapi.dev.log !TAG!"
call :note
goto :eof
:addfile
if not exist "%~1" goto :eof
copy /y "%~1" "%SystemRoot%\Temp\MacBookPatcher-command.txt" >nul
"%LOGGER%" --capture
goto :eof
:cue
if not exist %SystemRoot%\System32\a1706cue.exe (
  set "MSG=ERROR missing a1706cue.exe"
  call :note
  goto :eof
)
%SystemRoot%\System32\a1706cue.exe %~1
set RC=!ERRORLEVEL!
set "MSG=cue %~1 errorlevel=!RC!"
call :note
goto :eof
:note
"%LOGGER%" --event "!PHASE!" "!MSG!"
goto :eof
