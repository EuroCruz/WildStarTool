@echo off
setlocal enabledelayedexpansion
cd /d "%~dp0.."

set "TARGETS=%*"
if "%TARGETS%"=="" set "TARGETS=x86_64-pc-windows-gnullvm"

for /f "tokens=2 delims== " %%v in ('findstr /b "version" Cargo.toml') do set "VERSION=%%~v"

if exist dist rmdir /s /q dist

for %%T in (%TARGETS%) do (
    echo ==^> %%T
    cargo build --release --locked --target %%T || exit /b 1
    set "OUT=dist\wildstartool-%VERSION%-%%T"
    mkdir "!OUT!"
    copy /y "target\%%T\release\wildstartool.exe" "!OUT!\" >nul || exit /b 1
    xcopy /e /i /q /y licenses "!OUT!\licenses" >nul
    pushd "!OUT!"
    set "ROOT=!CD!\"
    (for /r %%F in (*) do (
        set "REL=%%F"
        for %%R in ("!ROOT!") do set "REL=!REL:%%~R=!"
        set "REL=!REL:\=/!"
        for /f "delims=" %%H in ('certutil -hashfile "%%F" SHA256 ^| findstr /v ":"') do echo %%H *!REL!
    )) > ..\SHA256SUMS
    move /y ..\SHA256SUMS SHA256SUMS >nul
    popd
)

echo ==^> dist
