@echo off
chcp 65001 > nul
title sconc v0.1 — 한국어 다층 말뭉치 통합 검색기
cd /d "%~dp0"

if not exist "engine\target\release\sconc.exe" (
    echo [안내] sconc 실행 파일이 없습니다. 초기 설치를 진행합니다...
    call install.bat
)

if exist "engine\target\release\sconc.exe" (
    "engine\target\release\sconc.exe" --port 8787 --open
) else (
    echo [오류] 실행 파일을 찾을 수 없습니다. install.bat을 다시 실행해 주세요.
    pause
)
