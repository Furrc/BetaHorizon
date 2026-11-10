@echo off
setlocal
cd /d "%~dp0"

if not defined OLDMINE_RENDER_DEBUG set "OLDMINE_RENDER_DEBUG=1"

cargo run --manifest-path "%~dp0Cargo.toml" -p oldmine-client -- %*
exit /b %errorlevel%
