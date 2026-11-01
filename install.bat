@echo off
setlocal enabledelayedexpansion
title sconc v0.1 - Environment Setup

echo ================================================================
echo       sconc v0.1: Environment Setup and Binary Installer
echo ================================================================
echo.

set "SCRIPT_DIR=%~dp0"
cd /d "%SCRIPT_DIR%"

echo [1/4] Checking Python 3 environment...
where python >nul 2>nul
if %errorlevel% neq 0 (
    echo [WARNING] Python 3 not found in PATH.
    echo Please install Python 3.8+ from https://www.python.org/downloads/
    echo.
) else (
    for /f "tokens=*" %%i in ('python --version 2^^^>^^^&1') do set "PY_VER=%%i"
    echo   - Detected: !PY_VER! [OK]
)
echo.

echo [2/4] Checking Rust / Cargo toolchain...
where cargo >nul 2>nul
if %errorlevel% neq 0 (
    if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
        set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
        echo   - Added Cargo path to PATH.
    ) else (
        echo [NOTICE] Rust toolchain not detected.
        echo Downloading rustup-init.exe...
        powershell -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile '%TEMP%\rustup-init.exe'"
        if exist "%TEMP%\rustup-init.exe" (
            echo Running rustup-init...
            "%TEMP%\rustup-init.exe" -y
            set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
        ) else (
            echo [ERROR] Failed to download rustup-init.exe. Please install from https://rustup.rs/
            pause
            exit /b 1
        )
    )
)
for /f "tokens=*" %%i in ('cargo --version 2^^^>^^^&1') do set "CARGO_VER=%%i"
echo   - Detected: !CARGO_VER! [OK]
echo.

echo [3/4] Building sconc release binary...
cargo build --release --manifest-path "engine\Cargo.toml"
if %errorlevel% neq 0 (
    echo [ERROR] Build failed.
    pause
    exit /b 1
)
echo   - Release binary ready: engine\target\release\sconc.exe [OK]
echo.

echo [4/4] Registering global command in User PATH...
set "GLOBAL_BIN_DIR=%USERPROFILE%\.cargo\bin"
if not exist "%GLOBAL_BIN_DIR%" mkdir "%GLOBAL_BIN_DIR%"

(
    echo @echo off
    echo set "SCONC_HOME=%SCRIPT_DIR%"
    echo "%SCRIPT_DIR%engine\target\release\sconc.exe" --port 8787 --open %%*
) > "%GLOBAL_BIN_DIR%\sconc.cmd"

powershell -Command " = [Environment]::GetEnvironmentVariable('Path', 'User');  = '%SCRIPT_DIR%'.TrimEnd('\'); if ( -notlike ('*' +  + '*')) { [Environment]::SetEnvironmentVariable('Path',  + ';' + , 'User') }"

echo   - Registered: %GLOBAL_BIN_DIR%\sconc.cmd
echo   - You can now run 'sconc' from any terminal directory.
echo.
echo ================================================================
echo       Setup completed successfully!
echo ================================================================
echo.
pause
