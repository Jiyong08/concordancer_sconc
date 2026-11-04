@echo off
setlocal
cd /d "%~dp0"
title sconc v0.1 - Korean Multi-layer Corpus Concordancer

if exist "sconc.exe" (
    "sconc.exe" --port 8787 --open
    goto :done
)

if exist "engine\target\release\sconc.exe" (
    "engine\target\release\sconc.exe" --port 8787 --open
    goto :done
)

if exist "engine\target\release\concordance-engine.exe" (
    "engine\target\release\concordance-engine.exe" --port 8787 --open
    goto :done
)

echo [sconc] Executable binary not found. Launching install/build...
if exist "install.bat" call install.bat
if exist "sconc.exe" (
    "sconc.exe" --port 8787 --open
    goto :done
)
if exist "engine\target\release\sconc.exe" (
    "engine\target\release\sconc.exe" --port 8787 --open
)

:done
endlocal
