@echo off
chcp 65001 > nul

:: ============================================================
:: Check for parameters first
:: ============================================================
if "%1"=="menu" goto :legacyMenu

:: ============================================================
:: Launch Modern UI (Default)
:: ============================================================
python "Z:\Closet\UI\main.py"
exit /b 0

:legacyMenu
cls
echo.
echo ============================================
echo  File Processing Tool
echo ============================================
echo.
echo 1. Full process (STEP 1-6)
echo 2. Rename only (remove _ and parentheses)
echo 3. Timestamp rename (select folder)
echo 4. Image to JXL conversion (lossless - JPG/HEIC/PNG/GIF/etc)
echo 5. Hash cache database (hash_cache_db.exe)
echo.

choice /C 12345 /M "Select an option"

if errorlevel 5 goto :hashCacheDb
if errorlevel 4 goto :jpgToJxl
if errorlevel 3 goto :timestampRename
if errorlevel 2 goto :renameOnly
if errorlevel 1 goto :fullProcess

:: ============================================================
:: Full Process Mode
:: ============================================================
:fullProcess
cls
echo.
echo [FULL PROCESS MODE]
echo.

:: ============================================================
:: Settings
:: ============================================================
set "TWITTER_SRC=Z:\gallery-dl\twitter\A_Quei_72"
set "DOWNLOAD_SRC=C:\Users\E1192\Downloads"
set "DEST=Z:\Pictures\Rename"
set "REF=Z:\R1"

:: ============================================================
:: Initialization
:: ============================================================
if not exist "%DEST%" mkdir "%DEST%"

:: ============================================================
:: STEP 1: Move all files from Twitter folder
:: ============================================================
echo [STEP 1] Moving all files from Twitter folder...
powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-ChildItem -LiteralPath '%TWITTER_SRC%' -File | ForEach-Object { Move-Item -LiteralPath $_.FullName -Destination '%DEST%' -Force }"
if errorlevel 1 goto :error

:: ============================================================
:: STEP 2: Move images from Downloads (last 7 days, top-level only)
:: ============================================================
echo [STEP 2] Moving recent images from Downloads...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ext=@('.jxl','.jpg','.jpeg','.png','.gif','.bmp'); $limit=(Get-Date).AddDays(-7).Date; Get-ChildItem -LiteralPath '%DOWNLOAD_SRC%' -File | Where-Object { $ext -contains $_.Extension.ToLower() -and $_.CreationTime.Date -ge $limit } | ForEach-Object { Move-Item -LiteralPath $_.FullName -Destination '%DEST%' -Force }"
if errorlevel 1 goto :error

:: ============================================================
:: STEP 2.5: Move images from Downloads\X_Images and delete folder
:: ============================================================
echo [STEP 2.5] Moving images from Downloads\X_Images...
set "X_IMAGES_SRC=%DOWNLOAD_SRC%\X_Images"
if exist "%X_IMAGES_SRC%" (
    powershell -NoProfile -ExecutionPolicy Bypass -Command "$ext=@('.jxl','.jpg','.jpeg','.png','.gif','.bmp'); Get-ChildItem -LiteralPath '%X_IMAGES_SRC%' -File | Where-Object { $ext -contains $_.Extension.ToLower() } | ForEach-Object { Move-Item -LiteralPath $_.FullName -Destination '%DEST%' -Force }"
    if errorlevel 1 goto :error

    echo [STEP 2.5] Deleting files in Downloads\X_Images...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-ChildItem -LiteralPath '%X_IMAGES_SRC%' -File | Remove-Item -Force"
    if errorlevel 1 goto :error

    echo [STEP 2.5] Checking if X_Images folder is empty and deleting if so...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "if ((Get-ChildItem -LiteralPath '%X_IMAGES_SRC%' -Force | Measure-Object).Count -eq 0) { Remove-Item -LiteralPath '%X_IMAGES_SRC%' -Force; Write-Host 'X_Images folder deleted' } else { Write-Host 'X_Images folder is not empty. Keeping it.' }"
    if errorlevel 1 goto :error
) else (
    echo [STEP 2.5] Downloads\X_Images folder not found. Skipping...
)

:: ============================================================
:: STEP 3: Remove duplicates within Z:\Rename (SHA256)
:: ============================================================
echo [STEP 3] Removing duplicates within %DEST%...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$seen = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::OrdinalIgnoreCase); Get-ChildItem -LiteralPath '%DEST%' -File | ForEach-Object { $h = (Get-FileHash $_.FullName -Algorithm SHA256).Hash; if (-not $seen.Add($h)) { Write-Host ('>> Removing duplicate: ' + $_.Name); Remove-Item -LiteralPath $_.FullName -Force } }"
if errorlevel 1 goto :error

:: ============================================================
:: STEP 4: Remove files in Z:\Rename that also exist in Z:\R1
:: ============================================================
echo [STEP 4] Removing files in %DEST% that exist in %REF%...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$refHashes = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::OrdinalIgnoreCase); if (Test-Path '%REF%') { Get-ChildItem -LiteralPath '%REF%' -File -Recurse | ForEach-Object { $refHashes.Add((Get-FileHash $_.FullName -Algorithm SHA256).Hash) | Out-Null } }; Get-ChildItem -LiteralPath '%DEST%' -File | ForEach-Object { $h = (Get-FileHash $_.FullName -Algorithm SHA256).Hash; if ($refHashes.Contains($h)) { Write-Host ('>> Removing (Exists in Ref): ' + $_.Name); Remove-Item -LiteralPath $_.FullName -Force } }"
if errorlevel 1 goto :error

:: ============================================================
:: STEP 5: Rename files by last modified timestamp
:: ============================================================
echo [STEP 5] Renaming files by timestamp...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$imgExt=@('.jxl','.jpg','.jpeg','.png','.gif','.bmp'); Get-ChildItem -LiteralPath '%DEST%' -File | Where-Object { $imgExt -contains $_.Extension.ToLower() -and $_.BaseName -notmatch '^[0-9]{14}' } | ForEach-Object { $b = $_.LastWriteTime.ToString('yyyyMMddHHmmss'); $ext = $_.Extension; $n = $b + $ext; while (Test-Path (Join-Path $_.DirectoryName $n)) { $r = Get-Random -Minimum 0 -Maximum 10; $n = $b + $r + $ext }; Write-Host ('>> Renaming: ' + $_.Name + ' -> ' + $n); Rename-Item -LiteralPath $_.FullName -NewName $n }"
if errorlevel 1 goto :error

:: ============================================================
:: STEP 6: Remove underscores and parentheses from filenames
:: (Handles files with patterns like "123_456.jpg" or "name (1).jpg")
:: ============================================================
echo [STEP 6] Removing underscores and parentheses from filenames...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$imgExt=@('.jxl','.jpg','.jpeg','.png','.gif','.bmp'); $count = 0; Get-ChildItem -LiteralPath '%DEST%' -File | Where-Object { $imgExt -contains $_.Extension.ToLower() } | ForEach-Object { $filename = [System.IO.Path]::GetFileNameWithoutExtension($_.Name); $ext = $_.Extension; $newname = $null; if ($filename -match '^\d+_\d+$') { $newname = ($filename -replace '_', '') + $ext } elseif ($filename -match ' \(\d+\)$') { $newname = ($filename -replace ' \(\d+\)$', '') + $ext }; if ($newname) { $targetPath = Join-Path $_.DirectoryName $newname; while (Test-Path $targetPath) { $base = [System.IO.Path]::GetFileNameWithoutExtension($newname); $ext2 = [System.IO.Path]::GetExtension($newname); $r = Get-Random -Minimum 0 -Maximum 10; $newname = $base + $r + $ext2; $targetPath = Join-Path $_.DirectoryName $newname }; Write-Host ('>> Renaming: ' + $_.Name + ' -> ' + $newname); Rename-Item -LiteralPath $_.FullName -NewName $newname; $count++ }; }; Write-Host ('Renamed ' + $count + ' files')"
if errorlevel 1 goto :error

:: ============================================================
:: STEP 7: Convert images to JXL (lossless)
:: ============================================================
echo [STEP 7] Converting images to JXL format...
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0jpg-to-jxl.ps1" -convertPath "%DEST%"
if errorlevel 1 goto :error

:: ============================================================
:: Success
:: ============================================================
echo.
echo All processes completed successfully!
timeout /t 10
exit /b 0

:: ============================================================
:: Timestamp Rename Mode
:: ============================================================
:timestampRename
cls
echo.
echo [TIMESTAMP RENAME MODE]
echo.
set /p timestampTarget="Enter the target folder path: "

if not exist "%timestampTarget%" (
    echo Error: Folder not found
    pause
    exit /b 1
)

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0timestamp-rename.ps1" -targetPath "%timestampTarget%"

timeout /t 10
exit /b 0

:: ============================================================
:: JPG to JXL Conversion Mode
:: ============================================================
:jpgToJxl
cls
echo.
echo [IMAGE TO JXL CONVERSION]
echo.

REM Check cjxl availability
where cjxl > nul 2>&1
if errorlevel 1 (
    echo Error: cjxl not installed
    echo Please install libjxl first
    echo.
    pause
    exit /b 1
)

set /p "convertPath=Enter path to process (e.g. Z:\images): "

if not exist "%convertPath%" (
    echo Error: Path not found
    pause
    exit /b 1
)

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0jpg-to-jxl.ps1" -convertPath "%convertPath%"
echo.
timeout /t 10
exit /b 0

:: ============================================================
:: Rename Only Mode
:: ============================================================
:renameOnly
cls
echo.
echo [RENAME ONLY MODE]
echo.
set /p renameTarget="Enter the target folder path: "

if not exist "%renameTarget%" (
    echo Error: Folder not found
    pause
    exit /b 1
)

echo.
echo Processing...
echo.

powershell -NoProfile -ExecutionPolicy Bypass -Command "$imgExt=@('.jxl','.jpg','.jpeg','.png','.gif','.bmp'); $count = 0; Get-ChildItem -LiteralPath '%renameTarget%' -File | Where-Object { $imgExt -contains $_.Extension.ToLower() } | ForEach-Object { $filename = [System.IO.Path]::GetFileNameWithoutExtension($_.Name); $ext = $_.Extension; $newname = $null; if ($filename -match '^\d+_\d+$') { $newname = ($filename -replace '_', '') + $ext } elseif ($filename -match ' \(\d+\)$') { $newname = ($filename -replace ' \(\d+\)$', '') + $ext }; if ($newname) { $targetPath = Join-Path $_.DirectoryName $newname; while (Test-Path $targetPath) { $base = [System.IO.Path]::GetFileNameWithoutExtension($newname); $ext2 = [System.IO.Path]::GetExtension($newname); $r = Get-Random -Minimum 0 -Maximum 10; $newname = $base + $r + $ext2; $targetPath = Join-Path $_.DirectoryName $newname }; Write-Host ('>> Renaming: ' + $_.Name + ' -> ' + $newname); Rename-Item -LiteralPath $_.FullName -NewName $newname; $count++ }; }; Write-Host ('Renamed ' + $count + ' files')"

echo.
echo Rename only process completed!
timeout /t 10
exit /b 0

:: ============================================================
:: Hash Cache Database Mode
:: ============================================================
:hashCacheDb
cls
echo.
echo [HASH CACHE DATABASE]
echo.
echo Running hash_cache_db.exe...
"Z:\Closet\Remove-Duplicates\hash_cache_db.exe"
echo.
echo Hash cache database process completed!
timeout /t 10
exit /b 0

:: ============================================================
:: Error Handler
:: ============================================================
:error
echo.
echo An error occurred. Please check the output above.
pause
exit /b 1