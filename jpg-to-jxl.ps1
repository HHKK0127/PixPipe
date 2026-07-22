param([string]$convertPath)

$imgExt = @('.jpg', '.JPG', '.jpeg', '.JPEG', '.png', '.PNG', '.gif', '.GIF', '.bmp', '.BMP', '.webp', '.WEBP', '.tiff', '.TIFF', '.tif', '.TIF')
$success = 0
$skip = 0
$failed = 0

if (-not (Test-Path $convertPath)) {
    Write-Host "Error: Path not found"
    exit 1
}

Write-Host ""
Write-Host "Processing directory: $convertPath"
Write-Host "Searching for JPG files..."
Write-Host ""

# Cleanup: Remove any remaining .jxl.hash files from previous incomplete runs
Write-Host "Cleaning up orphaned .jxl.hash files..."
Get-ChildItem -LiteralPath $convertPath -File -Recurse -Filter "*.jxl.hash" | ForEach-Object {
    Write-Host "  Removing: $($_.Name)"
    Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue
}
Write-Host ""

Get-ChildItem -LiteralPath $convertPath -File -Recurse | Where-Object { $imgExt -contains $_.Extension.ToLower() } | ForEach-Object {
    $jpgPath = $_.FullName
    $basePath = [System.IO.Path]::Combine($_.DirectoryName, [System.IO.Path]::GetFileNameWithoutExtension($_.Name))
    $jxlPath = $basePath + '.jxl'
    $hashPath = $jxlPath + '.hash'

    # Calculate hash of current JPG
    $jpgHash = (Get-FileHash $jpgPath -Algorithm SHA256).Hash

    if (Test-Path $jxlPath) {
        # JXL exists, compare with hash if available
        if (Test-Path $hashPath) {
            $savedHash = Get-Content -LiteralPath $hashPath -Raw -ErrorAction SilentlyContinue
            if ($savedHash -eq $jpgHash) {
                # Same content, JPG is duplicate
                Write-Host "  SKIP: $($_.Name) (already converted, same content)"
                Remove-Item -LiteralPath $jpgPath -Force
                Remove-Item -LiteralPath $hashPath -Force
                $script:skip++
            } else {
                # Different content, need to reconvert
                Write-Host "Processing: $($_.Name) (different from previous)"
                $reconvert = $true
            }
        } else {
            # No hash file, assume already converted
            Write-Host "  SKIP: $($_.Name) (already converted)"
            $script:skip++
            $reconvert = $false
        }
    } else {
        $reconvert = $true
    }

    if ($reconvert -or -not (Test-Path $jxlPath)) {
        Write-Host "Processing: $($_.Name)"

        $output = & cjxl "$jpgPath" "$jxlPath" -d 0 2>&1
        $exitCode = $LASTEXITCODE

        if ($exitCode -eq 0 -or (Test-Path $jxlPath)) {
            # Save hash for future comparison
            $jpgHash | Out-File -LiteralPath $hashPath -NoNewline -Force

            Remove-Item -LiteralPath $jpgPath -Force
            Remove-Item -LiteralPath $hashPath -Force
            $jxlName = Split-Path $jxlPath -Leaf
            Write-Host "  OK: $($_.Name) to $jxlName"
            $script:success++
        } else {
            # cjxl failed, try converting with ImageMagick first
            Write-Host "  Retrying with ImageMagick conversion..."
            $tempJpg = [System.IO.Path]::Combine($_.DirectoryName, [guid]::NewGuid().ToString() + '.jpg')

            $imgOutput = & magick "$jpgPath" -colorspace RGB "$tempJpg" 2>&1
            $imgExitCode = $LASTEXITCODE

            if ($imgExitCode -eq 0 -and (Test-Path $tempJpg)) {
                # Retry cjxl with converted image
                $retryOutput = & cjxl "$tempJpg" "$jxlPath" -d 0 2>&1
                $retryExitCode = $LASTEXITCODE

                if ($retryExitCode -eq 0 -or (Test-Path $jxlPath)) {
                    # Save hash for future comparison
                    $jpgHash | Out-File -LiteralPath $hashPath -NoNewline -Force

                    Remove-Item -LiteralPath $jpgPath -Force
                    Remove-Item -LiteralPath $tempJpg -Force
                    Remove-Item -LiteralPath $hashPath -Force
                    $jxlName = Split-Path $jxlPath -Leaf
                    Write-Host "  OK: $($_.Name) to $jxlName (after ImageMagick conversion)"
                    $script:success++
                } else {
                    Remove-Item -LiteralPath $tempJpg -Force
                    Write-Host "  Error: Conversion failed even after ImageMagick retry (exit code: $retryExitCode)"
                    $script:failed++
                }
            } else {
                Write-Host "  Error: ImageMagick conversion failed (exit code: $imgExitCode)"
                Write-Host "  Original cjxl error: $output"
                $script:failed++
            }
        }
    }
}

Write-Host ""
Write-Host "Completed"
Write-Host "Success: $success files"
Write-Host "Skipped: $skip files (already converted)"
Write-Host "Failed: $failed files"
