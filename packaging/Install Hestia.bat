@echo off
setlocal
title Install Hestia
set "DEST=%LOCALAPPDATA%\Programs\Hestia"
echo Installing Hestia...
taskkill /im hestia.exe /f >nul 2>&1
timeout /t 1 /nobreak >nul
if not exist "%DEST%" mkdir "%DEST%"
copy /y "%~dp0hestia.exe" "%DEST%\hestia.exe" >nul
if errorlevel 1 (
  echo Couldn't copy hestia.exe. Make sure you unzipped the folder first.
  pause
  exit /b 1
)
powershell -NoProfile -Command "$s=(New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('Programs')+'\Hestia.lnk'); $s.TargetPath='%DEST%\hestia.exe'; $s.WorkingDirectory='%DEST%'; $s.Description='Hestia launcher'; $s.Save()"
start "" "%DEST%\hestia.exe"
echo.
echo Hestia is installed! You'll find it in the Start menu.
echo The first time, a setup window helps you make it yours.
timeout /t 6 >nul
