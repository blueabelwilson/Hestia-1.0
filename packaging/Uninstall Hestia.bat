@echo off
setlocal
title Uninstall Hestia
echo Removing Hestia...
taskkill /im hestia.exe /f >nul 2>&1
timeout /t 1 /nobreak >nul
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v Hestia /f >nul 2>&1
del "%APPDATA%\Microsoft\Windows\Start Menu\Programs\Hestia.lnk" >nul 2>&1
rmdir /s /q "%LOCALAPPDATA%\Programs\Hestia" >nul 2>&1
echo.
choice /c YN /m "Also delete your Hestia settings (picture choice, colours, shortcuts)"
if errorlevel 2 goto done
rmdir /s /q "%APPDATA%\Hestia" >nul 2>&1
:done
echo.
echo Hestia has been removed.
timeout /t 5 >nul
