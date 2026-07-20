@echo off
cls
echo.
echo ========================================
echo     Z Drive Duplicate File Remover
echo ========================================
echo.
echo 1. hash_cache_db.exe を実行してからハッシュ削除
echo 2. ハッシュ削除を実行してから hash_cache_db.exe を実行
echo 3. ハッシュ削除のみ
echo.
set /p choice="選択してください (1-3): "

if "%choice%"=="1" (
    echo.
    echo hash_cache_db.exe を実行中...
    "Z:\Closet\Remove-Duplicates\hash_cache_db.exe"
    echo.
    echo ハッシュ重複削除を実行中...
    powershell -Command "Get-ChildItem -Path 'Z:' -File -Recurse | Get-FileHash -Algorithm MD5 | Group-Object -Property Hash | Where-Object { $_.Count -gt 1 } | ForEach-Object { $_.Group | Select-Object -Skip 1 } | Remove-Item -Force"
) else if "%choice%"=="2" (
    echo.
    echo ハッシュ重複削除を実行中...
    powershell -Command "Get-ChildItem -Path 'Z:' -File -Recurse | Get-FileHash -Algorithm MD5 | Group-Object -Property Hash | Where-Object { $_.Count -gt 1 } | ForEach-Object { $_.Group | Select-Object -Skip 1 } | Remove-Item -Force"
    echo.
    echo hash_cache_db.exe を実行中...
    "Z:\Closet\Remove-Duplicates\hash_cache_db.exe"
) else if "%choice%"=="3" (
    echo.
    echo ハッシュ重複削除を実行中...
    powershell -Command "Get-ChildItem -Path 'Z:' -File -Recurse | Get-FileHash -Algorithm MD5 | Group-Object -Property Hash | Where-Object { $_.Count -gt 1 } | ForEach-Object { $_.Group | Select-Object -Skip 1 } | Remove-Item -Force"
) else (
    echo 無効な選択です。
    timeout /t 3
    exit /b 1
)

echo.
timeout /t 10