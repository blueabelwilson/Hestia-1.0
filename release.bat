@echo off
cd /d "%~dp0"
title Building Hestia
echo Building the finished version of Hestia. This takes a few minutes...
echo.
cargo build --release
if errorlevel 1 (
  echo.
  echo The build failed.
  pause
  exit /b 1
)
if exist dist rmdir /s /q dist
mkdir "dist\Hestia"
copy /y "target\release\hestia.exe" "dist\Hestia\" >nul
copy /y "packaging\Install Hestia.bat" "dist\Hestia\" >nul
copy /y "packaging\Uninstall Hestia.bat" "dist\Hestia\" >nul
copy /y "packaging\Read me.txt" "dist\Hestia\" >nul
powershell -NoProfile -Command "Compress-Archive -Path 'dist\Hestia' -DestinationPath 'dist\Hestia.zip' -Force"
echo.
echo Done!
echo  - To install on this PC: open dist\Hestia and double-click "Install Hestia.bat"
echo  - To share with friends: send them dist\Hestia.zip
echo.
explorer "dist\Hestia"
pause
