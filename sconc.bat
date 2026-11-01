@echo off
setlocal
set "SCONC_ROOT=%~dp0"
"%SCONC_ROOT%engine\target\release\sconc.exe" --port 8787 --open %*
endlocal
