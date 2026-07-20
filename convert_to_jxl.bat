@echo off
chcp 65001 > /dev/null
setlocal enabledelayedexpansion

echo.
echo =====================================
echo  JPG to JXL Lossless Converter
echo =====================================
echo.

REM Check Z: drive
if not exist "Z:\" (
    echo Error: Z: drive not found
    pause
    exit /b 1
)

REM Input path from user
set /p "PROCESS_DIR=Enter path in Z: drive (e.g. Z:\images): "

REM Normalize path
if not "!PROCESS_DIR:~0,1!"=="\" (
    set "PROCESS_DIR=Z:\!PROCESS_DIR!"
)

REM Check directory exists
if not exist "!PROCESS_DIR!" (
    echo Error: Directory not found: !PROCESS_DIR!
    pause
    exit /b 1
)

echo.
echo Processing directory: !PROCESS_DIR!
echo.

REM Check cjxl availability
where cjxl > /dev/null 2>&1
if errorlevel 1 (
    echo Error: cjxl not installed
    echo Please install libjxl
    pause
    exit /b 1
)

echo Searching for JPG files...
setlocal enabledelayedexpansion
set "SUCCESS_COUNT=0"
set "FAIL_COUNT=0"

REM Process JPG files
for %%F in ("!PROCESS_DIR!\*.jpg" "!PROCESS_DIR!\*.JPG" "!PROCESS_DIR!\*.jpeg" "!PROCESS_DIR!\*.JPEG") do (
    if exist "%%F" (
        set "JPG_FILE=%%F"
        set "JXL_FILE=%%~dpnF.jxl"

        echo.
        echo Processing: %%~nxF

        REM Convert with cjxl (lossless)
        cjxl "!JPG_FILE!" "!JXL_FILE!" -d 0 -q 100

        if errorlevel 0 (
            REM Success - delete original
            del /q "!JPG_FILE!"
            if errorlevel 0 (
                echo   OK: %%~nxF to %%~nF.jxl
                set /a "SUCCESS_COUNT+=1"
            ) else (
                echo   Error: Failed to delete file
                set /a "FAIL_COUNT+=1"
            )
        ) else (
            echo   Error: Conversion failed
            set /a "FAIL_COUNT+=1"
        )
    )
)

echo.
echo =====================================
echo  Completed
echo =====================================
echo Success: !SUCCESS_COUNT! files
echo Failed: !FAIL_COUNT! files
echo.
pause
