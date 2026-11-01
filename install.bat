@echo off
setlocal enabledelayedexpansion
chcp 65001 > nul
title sconc v0.1 Installer ^& Environment Setup

echo ================================================================
echo       sconc v0.1: 한국어 다층 말뭉치 통합 검색기 환경 설치기
echo ================================================================
echo.

set "SCRIPT_DIR=%~dp0"
cd /d "%SCRIPT_DIR%"

:: 1. Python 설치 확인
echo [1/4] Python 3 환경 검사 중...
where python >nul 2>nul
if %errorlevel% neq 0 (
    echo [경고] Python 3가 설치되어 있지 않거나 PATH에 등록되지 않았습니다.
    echo 말뭉치 자동 변환기(corpus_builder.py) 구동을 위해 Python 3.8 이상 설치를 권장합니다.
    echo https://www.python.org/downloads/ 에서 설치해 주세요.
    echo.
) else (
    for /f "tokens=*" %%i in ('python --version 2^>^&1') do set "PY_VER=%%i"
    echo   - 감지된 파이썬 버전: !PY_VER! [OK]
)
echo.

:: 2. Rust / Cargo 설치 확인
echo [2/4] Rust / Cargo 빌드 툴체인 검사 중...
where cargo >nul 2>nul
if %errorlevel% neq 0 (
    if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
        set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
        echo   - Cargo 경로를 PATH에 임시 추가함.
    ) else (
        echo [안내] Rust(Cargo) 툴체인이 감지되지 않았습니다.
        echo Rust 공식 설치기(rustup-init.exe)를 다운로드하여 설치를 진행합니다...
        powershell -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile '%TEMP%\rustup-init.exe'"
        if exist "%TEMP%\rustup-init.exe" (
            echo rustup-init 실행 중... 기본 옵션(1)을 선택하여 설치해 주세요.
            "%TEMP%\rustup-init.exe" -y
            set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
        ) else (
            echo [오류] Rust 설치기 다운로드 실패. https://rustup.rs/ 에서 직접 설치해 주세요.
            pause
            exit /b 1
        )
    )
)
for /f "tokens=*" %%i in ('cargo --version 2^>^&1') do set "CARGO_VER=%%i"
echo   - 감지된 카고 버전: !CARGO_VER! [OK]
echo.

:: 3. sconc 엔진 빌드
echo [3/4] sconc Rust 릴리스 엔진 빌드 중...
cargo build --release --manifest-path "engine\Cargo.toml"
if %errorlevel% neq 0 (
    echo [오류] sconc 빌드 실패. 에러 메시지를 확인해 주세요.
    pause
    exit /b 1
)
echo   - 빌드 완료: engine\target\release\sconc.exe [OK]
echo.

:: 4. 어디서든 실행 가능하도록 PATH / 전역 명령어 등록
echo [4/4] 시스템 전역 실행 명령어 등록 중...
set "GLOBAL_BIN_DIR=%USERPROFILE%\.cargo\bin"
if not exist "%GLOBAL_BIN_DIR%" mkdir "%GLOBAL_BIN_DIR%"

(
    echo @echo off
    echo set "SCONC_HOME=%SCRIPT_DIR%"
    echo "%SCRIPT_DIR%engine\target\release\sconc.exe" --port 8787 --open %%*
) > "%GLOBAL_BIN_DIR%\sconc.cmd"

powershell -Command "$curr = [Environment]::GetEnvironmentVariable('Path', 'User'); $target = '%SCRIPT_DIR%'.TrimEnd('\'); if ($curr -notlike ('*' + $target + '*')) { [Environment]::SetEnvironmentVariable('Path', $curr + ';' + $target, 'User') }"

echo   - 전역 명령어 등록 완료: "%GLOBAL_BIN_DIR%\sconc.cmd" 및 사용자 PATH 등록
echo   - 이제 컴퓨터 어느 폴더의 명령 프롬프트나 터미널에서든 'sconc'를 입력하면 검색기가 실행됩니다!
echo.
echo ================================================================
echo       sconc v0.1 설치 및 환경 설정이 성공적으로 완료되었습니다!
echo       바로 실행하려면 'run_sconc.bat'을 실행하거나 터미널에서 'sconc'를 입력하세요.
echo ================================================================
echo.
pause
