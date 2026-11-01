@echo off
chcp 65001 > nul
title sconc v0.1 - 한국어 다층 말뭉치 통합 검색기
cd /d "%~dp0"

if exist "engine\target\release\sconc.exe" (
    "engine\target\release\sconc.exe" --port 8787 --open
) else if exist "engine\target\release\concordance-engine.exe" (
    "engine\target\release\concordance-engine.exe" --port 8787 --open
) else (
    echo [안내] 실행 바이너리를 빌드하거나 환경을 준비합니다...
    call install.bat
    if exist "engine\target\release\sconc.exe" (
        "engine\target\release\sconc.exe" --port 8787 --open
    )
)
pause
