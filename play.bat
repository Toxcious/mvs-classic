@echo off
rem Builds the launcher and the game if the code changed (a few seconds when nothing did), then opens the launcher
rem on the store imported in this repo, at its Home: PLAY starts the game. Extra arguments go to the launcher, and
rem the ones it does not know on to the game, e.g.
rem   play.bat --play                      (straight into the game, the launcher closing)
rem   play.bat --size 1920x1080 --no-audio
rem It builds the trace profile (release code, rebuilt incrementally: minutes faster after a change) on all
rem the processors at normal priority, since you wait on it.
cd /d "%~dp0"
echo Building the launcher and the game (only what changed)...
set MVS_CARGO_FOREGROUND=1
powershell -NoProfile -ExecutionPolicy Bypass -File tools\cargo-slot.ps1 build --profile trace -p mvs-app --bin mvs --bin mvs-launcher
if errorlevel 1 (
    echo.
    echo The build failed: the errors are above.
    pause
    exit /b 1
)
target\trace\mvs-launcher.exe --store content\.store --online-defaults online.json %*
if errorlevel 1 pause
