@echo off
chcp 65001 > nul

:: Launch improved Rust version with X_Images support
Z:\Closet\bat\io-optimized.exe %*
exit /b %errorlevel%
